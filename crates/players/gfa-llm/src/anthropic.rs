use crate::{
    ContentMessage, LlmProvider, PlayMode, ProviderError, ProviderFuture, ProviderRequest,
    ProviderResponse, Role, StopReason, ThinkingMode, Usage,
};
use reqwest::{header, Client};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fmt,
    io::Write,
    time::{Duration, Instant},
};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const CONTEXT_BETA: &str = "context-management-2025-06-27";

/// Anthropic Messages transport. Credentials stay in sensitive HTTP headers.
///
/// Calls have no automatic retries or redirects. The host must reserve budget
/// before every call, including retries, and retain reservations if billing is
/// unknown after cancellation or a transport error.
#[derive(Clone)]
pub struct AnthropicProvider {
    client: Client,
    endpoint: String,
}

impl fmt::Debug for AnthropicProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnthropicProvider")
            .finish_non_exhaustive()
    }
}

impl AnthropicProvider {
    /// Create a provider for the fixed HTTPS API endpoint.
    /// An absent or invalid key fails before any connection is made.
    pub fn new(api_key: &str) -> Result<Self, ProviderError> {
        if api_key.is_empty()
            || api_key.len() > 4096
            || api_key.bytes().any(|b| !b.is_ascii_graphic())
        {
            return Err(ProviderError::Authentication);
        }
        let mut key =
            header::HeaderValue::from_str(api_key).map_err(|_| ProviderError::Authentication)?;
        key.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert("x-api-key", key);
        headers.insert(
            "anthropic-version",
            header::HeaderValue::from_static("2023-06-01"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .no_proxy()
            .build()
            .map_err(|_| ProviderError::InvalidConfig)?;
        Ok(Self {
            client,
            endpoint: ENDPOINT.into(),
        })
    }

    async fn send(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let body = request_body(request)?;
        let started = Instant::now();
        let mut call = self
            .client
            .post(&self.endpoint)
            .timeout(Duration::from_millis(u64::from(
                request.config.move_timeout_ms,
            )))
            .body(body);
        if request.config.mode == PlayMode::Tools && request.config.context_editing {
            call = call.header("anthropic-beta", CONTEXT_BETA);
        }
        let mut response = call.send().await.map_err(transport_error)?;
        match response.status().as_u16() {
            200 => (),
            401 | 403 => return Err(ProviderError::Authentication),
            408 | 504 => return Err(ProviderError::Timeout),
            429 => return Err(ProviderError::RateLimited),
            400 | 404 | 413 | 422 => return Err(ProviderError::InvalidConfig),
            _ => return Err(ProviderError::Unavailable),
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BODY_BYTES as u64)
        {
            return Err(ProviderError::InvalidResponse);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if chunk.len() > MAX_BODY_BYTES - bytes.len() {
                return Err(ProviderError::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        let raw = serde_json::from_slice(&bytes).map_err(|_| ProviderError::InvalidResponse)?;
        let latency_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        parse_response(raw, latency_ms)
    }
}

impl LlmProvider for AnthropicProvider {
    fn complete<'a>(&'a self, request: &'a ProviderRequest) -> ProviderFuture<'a> {
        Box::pin(self.send(request))
    }
}

fn transport_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else {
        ProviderError::Unavailable
    }
}

fn request_body(request: &ProviderRequest) -> Result<Vec<u8>, ProviderError> {
    request.config.validate()?;
    if request.config.provider != "anthropic"
        || request.system.trim().is_empty()
        || request.system.len() > MAX_BODY_BYTES
        || request.messages.is_empty()
        || request.messages.len() > 4096
        || request
            .messages
            .first()
            .is_none_or(|message| message.role != Role::User)
        || request.messages.iter().any(|message| {
            message.content.is_empty()
                || message.content.iter().any(|block| {
                    block
                        .get("type")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                })
        })
        || request.tools.len() > 64
        || (request.config.mode == PlayMode::Direct && !request.tools.is_empty())
        || (request.config.mode == PlayMode::Tools && request.tools.is_empty())
    {
        return Err(ProviderError::InvalidConfig);
    }
    let mut names = HashSet::new();
    for tool in &request.tools {
        if tool.name.is_empty()
            || tool.name.len() > 64
            || !tool
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || !names.insert(&tool.name)
            || tool.description.trim().is_empty()
            || !tool.input_schema.is_object()
            || tool.input_schema["type"] != "object"
        {
            return Err(ProviderError::InvalidConfig);
        }
    }
    let mut body = json!({
        "model": request.config.model,
        "max_tokens": request.config.max_output_tokens,
        "stream": false,
        "system": [{"type":"text", "text":request.system, "cache_control":{"type":"ephemeral"}}],
        "messages": request.messages,
        "output_config": {"effort": request.config.effort},
        "thinking": match request.config.thinking {
            ThinkingMode::Adaptive => json!({"type":"adaptive", "display":"summarized"}),
            ThinkingMode::Disabled => json!({"type":"disabled"}),
        },
    });
    match request.config.mode {
        PlayMode::Direct => {
            body["output_config"]["format"] = json!({"type":"json_schema", "schema": {
                "type":"object", "properties": {
                    "reasoning":{"type":"string"}, "action":{"type":"string"}
                }, "required":["reasoning","action"], "additionalProperties":false
            }});
        }
        PlayMode::Tools => {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name":tool.name, "description":tool.description,
                            "input_schema":tool.input_schema, "strict":true,
                        })
                    })
                    .collect(),
            );
            body["tool_choice"] = json!({"type":"auto"});
            if request.config.context_editing {
                body["context_management"] = json!({"edits":[{
                    "type":"clear_tool_uses_20250919",
                    "trigger":{"type":"input_tokens","value":100_000},
                    "keep":{"type":"tool_uses","value":3},
                    "clear_tool_inputs":false,
                }]});
            }
        }
    }
    let mut output = LimitedBody(Vec::new());
    serde_json::to_writer(&mut output, &body).map_err(|_| ProviderError::InvalidConfig)?;
    Ok(output.0)
}

struct LimitedBody(Vec<u8>);
impl Write for LimitedBody {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_BODY_BYTES - self.0.len() {
            return Err(std::io::Error::other("request exceeds byte limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn counter(value: &Value, name: &str, optional: bool) -> Result<u64, ProviderError> {
    match value.get(name) {
        None | Some(Value::Null) if optional => Ok(0),
        Some(number) => number.as_u64().ok_or(ProviderError::InvalidResponse),
        None => Err(ProviderError::InvalidResponse),
    }
}
fn text_field(value: &Value, name: &str) -> Result<String, ProviderError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && text.len() <= 256)
        .map(str::to_owned)
        .ok_or(ProviderError::InvalidResponse)
}

fn parse_response(raw: Value, latency_ms: u64) -> Result<ProviderResponse, ProviderError> {
    if raw["type"] != "message" || raw["role"] != "assistant" || !raw["usage"].is_object() {
        return Err(ProviderError::InvalidResponse);
    }
    let content = raw["content"]
        .as_array()
        .ok_or(ProviderError::InvalidResponse)?;
    if content.iter().any(|block| {
        block
            .get("type")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    }) {
        return Err(ProviderError::InvalidResponse);
    }
    let usage = &raw["usage"];
    let writes = counter(usage, "cache_creation_input_tokens", true)?;
    let (cache_creation_5m_tokens, cache_creation_1h_tokens) = match usage.get("cache_creation") {
        None | Some(Value::Null) => (writes, 0),
        Some(detail) if detail.is_object() => {
            let short = counter(detail, "ephemeral_5m_input_tokens", false)?;
            let long = counter(detail, "ephemeral_1h_input_tokens", false)?;
            if short.checked_add(long) != Some(writes) {
                return Err(ProviderError::InvalidResponse);
            }
            (short, long)
        }
        _ => return Err(ProviderError::InvalidResponse),
    };
    let usage = Usage {
        input_tokens: counter(usage, "input_tokens", false)?,
        output_tokens: counter(usage, "output_tokens", false)?,
        cache_read_input_tokens: counter(usage, "cache_read_input_tokens", true)?,
        cache_creation_5m_tokens,
        cache_creation_1h_tokens,
    };
    usage.total()?;
    let stop_reason = match text_field(&raw, "stop_reason")?.as_str() {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "refusal" => StopReason::Refusal,
        "pause_turn" => StopReason::PauseTurn,
        other => StopReason::Other(other.into()),
    };
    Ok(ProviderResponse {
        id: text_field(&raw, "id")?,
        model: text_field(&raw, "model")?,
        message: ContentMessage {
            role: Role::Assistant,
            content: content.clone(),
        },
        usage,
        stop_reason,
        latency_ms,
        raw,
    })
}

#[cfg(test)]
mod tests;

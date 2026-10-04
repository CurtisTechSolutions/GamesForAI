use super::*;
use crate::{Effort, PlayerConfig, ToolDefinition};
use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::post,
    Router,
};
use std::sync::Arc;
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

type TestResult = Result<(), Box<dyn std::error::Error>>;
#[derive(Clone)]
struct MockState {
    captured: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    reply: Arc<dyn Fn() -> Response + Send + Sync>,
}
struct Mock {
    provider: AnthropicProvider,
    state: MockState,
    server: JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Mock {
    async fn new(
        reply: impl Fn() -> Response + Send + Sync + 'static,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let state = MockState {
            captured: Arc::default(),
            reply: Arc::new(reply),
        };
        let router = Router::new()
            .route(
                "/v1/messages",
                post(
                    |State(state): State<MockState>, request: Request| async move {
                        let (parts, body) = request.into_parts();
                        let bytes = axum::body::to_bytes(body, MAX_BODY_BYTES + 1024)
                            .await
                            .unwrap_or_default();
                        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                        state.captured.lock().await.push((parts.headers, body));
                        (state.reply)()
                    },
                ),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}/v1/messages", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let mut provider = AnthropicProvider::new("test-secret-never-log")?;
        provider.endpoint = endpoint;
        Ok(Self {
            provider,
            state,
            server,
        })
    }
    async fn json(value: Value) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new(move || Response::new(Body::from(value.to_string()))).await
    }
}
fn request(mode: PlayMode) -> ProviderRequest {
    ProviderRequest {
        config: PlayerConfig {
            mode,
            ..PlayerConfig::default()
        },
        system: "Immutable game briefing and prompt version.".into(),
        messages: vec![ContentMessage {
            role: Role::User,
            content: vec![json!({"type":"text","text":"Choose a legal move."})],
        }],
        tools: if mode == PlayMode::Tools {
            vec![ToolDefinition {
                name: "make_move".into(),
                description: "Play one legal move.".into(),
                input_schema: json!({"type":"object","properties":{"action":{"type":"string"}},"required":["action"],"additionalProperties":false}),
            }]
        } else {
            vec![]
        },
    }
}
fn completion() -> Value {
    json!({
        "id":"msg_test", "type":"message", "model":"configured-model-snapshot", "role":"assistant",
        "content":[
            {"type":"thinking","thinking":"A concise move summary.","signature":"opaque-signature"},
            {"type":"redacted_thinking","data":"opaque-redacted-payload"},
            {"type":"text","text":"{\"reasoning\":\"Control the center.\",\"action\":\"r2c2\"}"}
        ],
        "stop_reason":"end_turn", "stop_sequence":null,
        "usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":1000,
            "cache_creation_input_tokens":500,"cache_creation":{"ephemeral_5m_input_tokens":200,"ephemeral_1h_input_tokens":300}},
        "future_field":{"retained":true}
    })
}

#[tokio::test]
async fn direct_round_trip_preserves_blocks_and_accounts_cache_without_double_counting(
) -> TestResult {
    let raw = completion();
    let mock = Mock::json(raw.clone()).await?;
    let answer = mock.provider.complete(&request(PlayMode::Direct)).await?;
    assert_eq!(answer.raw, raw);
    assert_eq!(
        answer.message.content,
        raw["content"].as_array().ok_or("missing blocks")?.clone()
    );
    assert_eq!(answer.model, "configured-model-snapshot");
    assert_eq!(answer.usage.total()?, 1650);
    assert_eq!(answer.usage.cache_creation_1h_tokens, 300);
    assert_eq!(answer.stop_reason, StopReason::EndTurn);
    let captured = mock.state.captured.lock().await;
    assert_eq!(captured.len(), 1);
    let (headers, body) = &captured[0];
    assert_eq!(headers["x-api-key"], "test-secret-never-log");
    assert_eq!(headers["anthropic-version"], "2023-06-01");
    assert!(!headers.contains_key("anthropic-beta"));
    assert_eq!(body["output_config"]["effort"], "medium");
    assert_eq!(
        body["thinking"],
        json!({"type":"adaptive","display":"summarized"})
    );
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(
        body["output_config"]["format"]["schema"]["required"],
        json!(["reasoning", "action"])
    );
    assert_eq!(
        body["output_config"]["format"]["schema"]["additionalProperties"],
        false
    );
    assert!(body.get("tool_choice").is_none());
    assert!(body.get("tools").is_none());
    assert!(!format!("{:?}", mock.provider).contains("test-secret"));
    Ok(())
}

#[tokio::test]
async fn tools_use_strict_auto_choice_and_server_context_editing_without_editing_history(
) -> TestResult {
    let mut raw = completion();
    raw["stop_reason"] = json!("tool_use");
    raw["content"] =
        json!([{"type":"tool_use","id":"call_2","name":"make_move","input":{"action":"r2c2"}}]);
    let mock = Mock::json(raw).await?;
    let mut request = request(PlayMode::Tools);
    request.messages.push(ContentMessage {
        role: Role::Assistant,
        content: vec![
            json!({"type":"thinking","thinking":"Prior summary","signature":"unchanged-signature"}),
            json!({"type":"tool_use","id":"call_1","name":"make_move","input":{"action":"r1c1"}}),
        ],
    });
    request.messages.push(ContentMessage {
        role: Role::User,
        content: vec![
            json!({"type":"tool_result","tool_use_id":"call_1","content":"illegal action"}),
        ],
    });
    let original = request.clone();
    let answer = mock.provider.complete(&request).await?;
    assert_eq!(answer.stop_reason, StopReason::ToolUse);
    assert_eq!(request, original);
    let captured = mock.state.captured.lock().await;
    let (headers, body) = &captured[0];
    assert_eq!(headers["anthropic-beta"], CONTEXT_BETA);
    assert_eq!(body["messages"], serde_json::to_value(&original.messages)?);
    assert_eq!(body["tools"][0]["strict"], true);
    assert_eq!(body["tool_choice"], json!({"type":"auto"}));
    assert_eq!(
        body["context_management"]["edits"][0]["type"],
        "clear_tool_uses_20250919"
    );
    assert!(body["output_config"].get("format").is_none());
    Ok(())
}

#[tokio::test]
async fn optional_model_capabilities_can_be_disabled_and_change_identity() -> TestResult {
    let mock = Mock::json(completion()).await?;
    let mut request = request(PlayMode::Tools);
    let identity = request.config.agent_identity(false, true)?;
    request.config.thinking = ThinkingMode::Disabled;
    assert_ne!(identity, request.config.agent_identity(false, true)?);
    let identity = request.config.agent_identity(false, true)?;
    request.config.context_editing = false;
    assert_ne!(identity, request.config.agent_identity(false, true)?);
    request.config.effort = Effort::High;
    mock.provider.complete(&request).await?;
    let captured = mock.state.captured.lock().await;
    let (headers, body) = &captured[0];
    assert!(!headers.contains_key("anthropic-beta"));
    assert!(body.get("context_management").is_none());
    assert_eq!(body["thinking"], json!({"type":"disabled"}));
    assert_eq!(body["output_config"]["effort"], "high");
    Ok(())
}

#[tokio::test]
async fn http_errors_never_retry_or_expose_response_bodies() -> TestResult {
    for (status, expected) in [
        (400, ProviderError::InvalidConfig),
        (401, ProviderError::Authentication),
        (403, ProviderError::Authentication),
        (408, ProviderError::Timeout),
        (413, ProviderError::InvalidConfig),
        (429, ProviderError::RateLimited),
        (500, ProviderError::Unavailable),
        (529, ProviderError::Unavailable),
    ] {
        let mock = Mock::new(move || {
            let mut response = Response::new(Body::from("sensitive provider diagnostics"));
            *response.status_mut() =
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            response
        })
        .await?;
        let error = mock
            .provider
            .complete(&request(PlayMode::Direct))
            .await
            .err()
            .ok_or("expected failure")?;
        assert_eq!(error, expected);
        assert!(!format!("{error:?}: {error}").contains("sensitive"));
        assert_eq!(mock.state.captured.lock().await.len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn redirects_cannot_forward_provider_credentials() -> TestResult {
    let target = Mock::json(completion()).await?;
    let location = header::HeaderValue::from_str(&target.provider.endpoint)?;
    let mock = Mock::new(move || {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::TEMPORARY_REDIRECT;
        response
            .headers_mut()
            .insert(header::LOCATION, location.clone());
        response
    })
    .await?;
    assert_eq!(
        mock.provider.complete(&request(PlayMode::Direct)).await,
        Err(ProviderError::Unavailable)
    );
    assert_eq!(mock.state.captured.lock().await.len(), 1);
    assert!(target.state.captured.lock().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn response_body_reads_have_a_deadline_and_future_can_be_cancelled() -> TestResult {
    let mock = Mock::new(|| {
        let stream = futures_util::stream::once(async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Ok::<_, std::io::Error>(Bytes::from(completion().to_string()))
        });
        Response::new(Body::from_stream(stream))
    })
    .await?;
    let mut request = request(PlayMode::Direct);
    request.config.move_timeout_ms = 50;
    assert_eq!(
        mock.provider.complete(&request).await,
        Err(ProviderError::Timeout)
    );
    request.config.move_timeout_ms = 10_000;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), mock.provider.complete(&request))
            .await
            .is_err()
    );
    assert_eq!(mock.state.captured.lock().await.len(), 2);
    Ok(())
}

#[tokio::test]
async fn known_length_and_chunked_responses_cannot_exceed_the_byte_limit() -> TestResult {
    let mock = Mock::new(|| Response::new(Body::from(vec![b'x'; MAX_BODY_BYTES + 1]))).await?;
    assert_eq!(
        mock.provider.complete(&request(PlayMode::Direct)).await,
        Err(ProviderError::InvalidResponse)
    );
    let mock = Mock::new(|| {
        let chunks = (0..5).map(|_| Ok::<_, std::io::Error>(Bytes::from(vec![b'x'; 1024 * 1024])));
        Response::new(Body::from_stream(futures_util::stream::iter(chunks)))
    })
    .await?;
    assert_eq!(
        mock.provider.complete(&request(PlayMode::Direct)).await,
        Err(ProviderError::InvalidResponse)
    );
    Ok(())
}

#[tokio::test]
async fn invalid_and_oversized_requests_fail_without_an_http_call() -> TestResult {
    let mock = Mock::json(completion()).await?;
    let mut variants = vec![];
    let mut invalid = request(PlayMode::Direct);
    invalid.config.provider = "other".into();
    variants.push(invalid);
    let mut invalid = request(PlayMode::Direct);
    invalid.messages.clear();
    variants.push(invalid);
    let mut invalid = request(PlayMode::Tools);
    invalid.tools.push(invalid.tools[0].clone());
    variants.push(invalid);
    let mut invalid = request(PlayMode::Tools);
    invalid.config.mode = PlayMode::Direct;
    variants.push(invalid);
    let mut invalid = request(PlayMode::Direct);
    invalid.messages[0].content = vec![json!({"type":"text","text":"x".repeat(MAX_BODY_BYTES)})];
    variants.push(invalid);
    for invalid in variants {
        assert_eq!(
            mock.provider.complete(&invalid).await,
            Err(ProviderError::InvalidConfig)
        );
    }
    assert!(mock.state.captured.lock().await.is_empty());
    for key in ["", " ", "key\r\nheader: injected", "nonascii-ñ"] {
        assert!(matches!(
            AnthropicProvider::new(key),
            Err(ProviderError::Authentication)
        ));
    }
    Ok(())
}

#[test]
fn refusal_limits_and_unknown_stop_reasons_are_completions() -> TestResult {
    for (reason, expected) in [
        ("refusal", StopReason::Refusal),
        ("max_tokens", StopReason::MaxTokens),
        ("pause_turn", StopReason::PauseTurn),
        ("future_reason", StopReason::Other("future_reason".into())),
    ] {
        let mut raw = completion();
        raw["stop_reason"] = json!(reason);
        raw["content"] = json!([]);
        let result = parse_response(raw, 42)?;
        assert_eq!(result.stop_reason, expected);
        assert_eq!(result.latency_ms, 42);
        assert_eq!(result.usage.total()?, 1650);
    }
    Ok(())
}

#[tokio::test]
async fn malformed_json_and_inconsistent_usage_are_rejected() -> TestResult {
    let mock = Mock::new(|| Response::new(Body::from("not JSON"))).await?;
    assert_eq!(
        mock.provider.complete(&request(PlayMode::Direct)).await,
        Err(ProviderError::InvalidResponse)
    );
    for (pointer, replacement) in [
        ("/usage/input_tokens", json!(-1)),
        ("/usage/output_tokens", Value::Null),
        (
            "/usage/cache_creation/ephemeral_5m_input_tokens",
            json!(201),
        ),
        (
            "/usage/cache_creation/ephemeral_1h_input_tokens",
            json!(u64::MAX),
        ),
        ("/usage/input_tokens", json!(u64::MAX)),
        ("/role", json!("user")),
        ("/type", json!("error")),
        ("/content", json!([{"text":"missing type"}])),
        ("/id", json!("")),
        ("/stop_reason", Value::Null),
    ] {
        let mut raw = completion();
        *raw.pointer_mut(pointer).ok_or("missing fixture path")? = replacement;
        assert_eq!(parse_response(raw, 0), Err(ProviderError::InvalidResponse));
    }
    let mut raw = completion();
    raw["usage"] = json!({"input_tokens":2,"output_tokens":3});
    assert_eq!(parse_response(raw.clone(), 0)?.usage.total()?, 5);
    raw["usage"]["cache_creation_input_tokens"] = json!(7);
    let answer = parse_response(raw, 0)?;
    assert_eq!(answer.usage.cache_creation_5m_tokens, 7);
    assert_eq!(answer.usage.total()?, 12);
    Ok(())
}

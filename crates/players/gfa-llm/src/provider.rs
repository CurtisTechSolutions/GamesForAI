use crate::{PlayerConfig, Usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{future::Future, pin::Pin};

/// Errors never carry credentials or arbitrary HTTP response bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum ProviderError {
    /// Invalid host or player settings.
    #[error("invalid LLM configuration")]
    InvalidConfig,
    /// Provider credentials are absent or rejected.
    #[error("LLM provider authentication unavailable")]
    Authentication,
    /// Temporary transport or provider failure.
    #[error("LLM provider unavailable")]
    Unavailable,
    /// Provider rate limiting.
    #[error("LLM provider rate limited")]
    RateLimited,
    /// Wall time expired.
    #[error("LLM request timed out")]
    Timeout,
    /// Malformed, unsupported, or oversized response.
    #[error("invalid LLM provider response")]
    InvalidResponse,
    /// Host spending or token limit reached.
    #[error("LLM budget exhausted")]
    BudgetExhausted,
}

/// A conversation role; system instructions have a separate immutable field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Observation, tool result, or user request.
    User,
    /// Provider output, including opaque thinking signatures.
    Assistant,
}

/// Complete content blocks retained verbatim for append-only provider history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentMessage {
    /// Message author.
    pub role: Role,
    /// Provider content blocks; never reconstruct or edit signed thinking blocks.
    pub content: Vec<Value>,
}

/// A host-authorized tool with a strict input schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,
    /// Model-oriented usage instructions.
    pub description: String,
    /// JSON Schema accepted by the provider.
    pub input_schema: Value,
}

/// Credential-free request retained for replay and audit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequest {
    /// Full player settings, including explicit effort and response limit.
    pub config: PlayerConfig,
    /// Immutable briefing and prompt-template instructions.
    pub system: String,
    /// Append-only conversation.
    pub messages: Vec<ContentMessage>,
    /// Immutable authorized tools, empty in direct mode.
    pub tools: Vec<ToolDefinition>,
}

/// A normal provider completion may still fail to produce a valid game move.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Completed response.
    EndTurn,
    /// One or more tool invocations.
    ToolUse,
    /// Output limit reached; the caller must count a failed move attempt.
    MaxTokens,
    /// Provider refusal; the caller must count a failed move attempt.
    Refusal,
    /// Provider paused a long tool execution.
    PauseTurn,
    /// Another provider stop reason, preserved for diagnostics.
    Other(String),
}

/// Provider response and usage; price is calculated from the host pricing record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderResponse {
    /// Provider request/message identifier.
    pub id: String,
    /// Model actually reported by the provider.
    pub model: String,
    /// Complete assistant message for append-only replay.
    pub message: ContentMessage,
    /// Billing counters reported by the provider.
    pub usage: Usage,
    /// Completion status, including unsuccessful move attempts.
    pub stop_reason: StopReason,
    /// Measured request wall time.
    pub latency_ms: u64,
    /// Original credential-free response for audit and forward compatibility.
    pub raw: Value,
}

/// Object-safe asynchronous provider call.
pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderError>> + Send + 'a>>;

/// A provider holds its own credentials and transport, outside serializable match data.
/// Dropping the future must cancel the local request; remote billing may still occur.
pub trait LlmProvider: Send + Sync {
    /// Complete a bounded request. The host reserves budget before calling this.
    fn complete<'a>(&'a self, request: &'a ProviderRequest) -> ProviderFuture<'a>;
}

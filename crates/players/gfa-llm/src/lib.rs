//! LLM player contracts, integer accounting, and bounded Anthropic HTTP requests.
//! Credentials belong to a provider implementation, never these serializable types.
mod accounting;
mod anthropic;
mod config;
mod provider;
mod runner;

pub use accounting::{Budget, Pricing, Usage};
pub use anthropic::AnthropicProvider;
pub use config::{Effort, PlayMode, PlayerConfig, ThinkingMode};
pub use provider::{
    ContentMessage, LlmProvider, ProviderError, ProviderFuture, ProviderRequest, ProviderResponse,
    Role, StopReason, ToolDefinition,
};
pub use runner::{
    AllowedAssists, CallRecord, FailedAttempt, MoveDecision, PlayerSession, ToolExchange,
    ToolFuture, ToolResult, TurnContext, TurnFailure, TurnOutcome, TurnReport, TurnTools,
};

#[cfg(test)]
mod tests;

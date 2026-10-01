//! Provider-neutral LLM player configuration, requests, and integer accounting.
//! Credentials belong to a provider implementation, never these serializable types.
mod accounting;
mod config;
mod provider;

pub use accounting::{Budget, Pricing, Usage};
pub use config::{Effort, PlayMode, PlayerConfig};
pub use provider::{
    ContentMessage, LlmProvider, ProviderError, ProviderFuture, ProviderRequest, ProviderResponse,
    Role, StopReason, ToolDefinition,
};

#[cfg(test)]
mod tests;

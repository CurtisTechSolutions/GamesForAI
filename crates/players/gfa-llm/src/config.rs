use crate::ProviderError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How the player chooses its next action.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    /// Interact through the host's authorized game tools.
    #[default]
    Tools,
    /// Return one structured action without tool calls.
    Direct,
}

/// Explicit provider reasoning effort, recorded as part of agent identity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    /// Minimum effort.
    Low,
    /// Balanced effort.
    #[default]
    Medium,
    /// Increased effort.
    High,
    /// Extra high effort on supported models.
    Xhigh,
    /// Highest effort on supported models.
    Max,
}

/// Serializable settings; there is deliberately no API key field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerConfig {
    /// Registry provider name.
    pub provider: String,
    /// Provider model identifier.
    pub model: String,
    /// Direct action or authorized tool interaction.
    pub mode: PlayMode,
    /// Version of the immutable prompt template.
    pub prompt_version: String,
    /// Explicit reasoning effort.
    pub effort: Effort,
    /// Hard output-token limit per API response, including thinking.
    pub max_output_tokens: u32,
    /// Total input, output, and cache tokens allowed for a game.
    pub max_game_tokens: u64,
    /// Maximum game cost in millionths of one USD.
    pub max_game_cost_microusd: u64,
    /// Wall time allowed for one complete move attempt.
    pub move_timeout_ms: u32,
    /// Additional attempts after an invalid action or unsuccessful completion.
    pub illegal_move_retries: u8,
}
impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into(),
            mode: PlayMode::Tools,
            prompt_version: "gfa.llm.1".into(),
            effort: Effort::Medium,
            max_output_tokens: 4096,
            max_game_tokens: 250_000,
            max_game_cost_microusd: 1_000_000,
            move_timeout_ms: 60_000,
            illegal_move_retries: 2,
        }
    }
}
fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}
impl PlayerConfig {
    /// Validate limits before allocating work or contacting a provider.
    pub fn validate(&self) -> Result<(), ProviderError> {
        if !identifier(&self.provider, 64)
            || !identifier(&self.model, 128)
            || !identifier(&self.prompt_version, 64)
            || !(1..=128_000).contains(&self.max_output_tokens)
            || self.max_game_tokens == 0
            || self.max_game_cost_microusd == 0
            || !(1..=600_000).contains(&self.move_timeout_ms)
            || self.illegal_move_retries > 10
        {
            return Err(ProviderError::InvalidConfig);
        }
        Ok(())
    }

    /// Stable identity covering all configuration and strength-changing assists.
    /// Host secrets and provider pricing never contribute to this fingerprint.
    pub fn agent_identity(
        &self,
        allow_analysis: bool,
        allow_simulation: bool,
    ) -> Result<String, ProviderError> {
        self.validate()?;
        let bytes = serde_json::to_vec(&(1_u8, self, allow_analysis, allow_simulation))
            .map_err(|_| ProviderError::InvalidConfig)?;
        let digest = Sha256::digest(bytes);
        Ok(format!("llm:{}/{}/{digest:x}", self.provider, self.model))
    }
}

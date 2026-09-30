//! Validated position import/export contracts.
use crate::{empty_config, Start};
use gfa_core::{LegalAction, Observation, PlayerId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Import an explicit position without creating or changing a match.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidatePosition {
    /// Configuration used to parse the position.
    #[serde(default = "empty_config")]
    pub config: Value,
    /// Position notation or complete structured state.
    pub start: Start,
    /// Fresh RNG seed; omitted seeds come from the host's entropy provider.
    #[serde(default)]
    pub seed: Option<u64>,
}

/// All encodings of a validated standalone position, including finished boards.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidatedPosition {
    /// Registered game identifier.
    pub game_id: String,
    /// Engine version used for validation.
    pub engine_version: String,
    /// Configuration with defaults materialized.
    pub config: Value,
    /// Seed installed for future chance events.
    pub seed: u64,
    /// Canonical position notation.
    pub position: String,
    /// Complete state for this caller-supplied standalone position.
    pub state: Value,
    /// Current actors; empty for a finished position.
    pub to_act: Vec<PlayerId>,
    /// Requested viewer's board, including any tensor encoding.
    pub observation: Observation,
    /// Current actions for the requested seat.
    pub legal_actions: Vec<LegalAction>,
    /// Fixed discrete action mask for the requested seat.
    pub action_mask: Vec<bool>,
    /// Current per-seat returns.
    pub returns: Vec<f64>,
    /// Whether rules have ended play.
    pub terminated: bool,
}

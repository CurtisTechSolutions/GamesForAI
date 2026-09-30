//! Bounded planning requests, assist policy and accounting.
use crate::{empty_config, ApiError, MatchState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn yes() -> bool {
    true
}

/// Assistance permitted by a match, recorded for reproducible evaluation.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assists {
    /// Permit server-side hypothetical continuations.
    #[serde(default = "yes")]
    pub allow_simulation: bool,
    /// Permit engine analysis once an analysis provider is installed.
    #[serde(default)]
    pub allow_analysis: bool,
}

impl Default for Assists {
    fn default() -> Self {
        Self {
            allow_simulation: true,
            allow_analysis: false,
        }
    }
}

/// Initial source for independent hypothetical lines.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum SimulationFrom {
    /// A match observation at its current or an earlier turn.
    Match {
        /// Match whose information set is used.
        match_id: String,
        /// Acting viewer's authorized seat.
        seat: u8,
        /// Optional historical turn, defaulting to current.
        #[serde(default)]
        turn: Option<u64>,
    },
    /// An explicit position in engine notation.
    Position {
        /// Caller-supplied notation.
        position: String,
    },
    /// A complete state supplied by the caller.
    State {
        /// Caller-supplied state, including hidden fields if known.
        state: Value,
    },
}

/// Which hypothetical observations to return.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SimulationOutput {
    /// The final legal prefix, including the starting position for an empty line.
    #[default]
    Final,
    /// One state after each accepted move.
    All,
}

/// Up to sixteen independent lines of at most thirty-two actions each.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimulateRequest {
    /// Match observation or caller-supplied position.
    pub from: SimulationFrom,
    /// Configuration for standalone positions. Match sources use recorded config.
    #[serde(default = "empty_config")]
    pub config: Value,
    /// Actions for each current player in sequential turn order.
    pub lines: Vec<Vec<Value>>,
    /// Independent chance seed; omitted seeds come from the host.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Final state or every accepted transition.
    #[serde(default, rename = "return")]
    pub output: SimulationOutput,
}

/// The first rejected action in one independent line.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SimulationFailure {
    /// Zero-based action offset.
    pub index: usize,
    /// Recoverable error; earlier accepted transitions remain in this line's result.
    pub error: ApiError,
}

/// One line's projected result; no live match has been changed.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SimulatedLine {
    /// Accepted transitions before completion or the first rejection.
    pub moves_applied: u32,
    /// Requested final or per-action observations with an empty match_id.
    pub states: Vec<MatchState>,
    /// First rejected action, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulationFailure>,
}

/// Independent line results in request order.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SimulationResult {
    /// Registered game identifier.
    pub game_id: String,
    /// Independent seed used for each line.
    pub seed: u64,
    /// Results in the same order as the requested lines.
    pub lines: Vec<SimulatedLine>,
}

/// Persistent assistance counters for one seat.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AssistUsage {
    /// Seat using the planning tools.
    pub seat: u8,
    /// Number of completed simulation requests, including lines with rejected moves.
    pub simulation_calls: u64,
    /// Number of successfully simulated transitions across those requests.
    pub simulated_moves: u64,
}

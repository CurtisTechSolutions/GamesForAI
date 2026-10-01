use crate::ApiError;
use gfa_core::{EnvSnapshot, LegalAction, Observation, PlayerId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Caller-owned training operations; these never address persisted matches.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrainingOperation {
    /// Create a seeded environment, optionally from custom notation.
    Create {
        /// Installed game identifier.
        game_id: String,
        /// Validated game options.
        #[serde(default = "crate::empty_config")]
        config: Value,
        /// Explicit deterministic seed.
        seed: u64,
        /// Optional custom start.
        #[serde(default)]
        position: Option<String>,
        /// Seat whose observation is returned.
        seat: PlayerId,
    },
    /// Reset a compatible checkpoint to a new seed or position.
    Reset {
        /// Caller-owned private checkpoint.
        checkpoint: EnvSnapshot,
        /// New episode seed.
        seed: u64,
        /// Optional custom start.
        #[serde(default)]
        position: Option<String>,
        /// Seat whose observation is returned.
        seat: PlayerId,
    },
    /// Apply one discrete action to a private checkpoint.
    Step {
        /// Caller-owned private checkpoint.
        checkpoint: EnvSnapshot,
        /// Acting seat, also used for the response projection.
        seat: PlayerId,
        /// Index in the game's fixed action space.
        action: u32,
    },
    /// Validate a checkpoint and project a seat without stepping.
    Observe {
        /// Caller-owned private checkpoint.
        checkpoint: EnvSnapshot,
        /// Seat whose observation is returned; omit for a public spectator projection.
        #[serde(default)]
        seat: Option<PlayerId>,
    },
}

/// Bounded, ordered training batch, with independent results per input.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrainingBatch {
    /// One to 64 independent operations.
    pub operations: Vec<TrainingOperation>,
}

/// One seat-scoped observation and global episode bookkeeping.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TrainingFrame {
    /// Selected seat, or None for a public spectator projection.
    pub seat: Option<PlayerId>,
    /// Only that seat's visible information.
    pub observation: Observation,
    /// Seat-scoped legal catalog.
    pub legal_actions: Vec<LegalAction>,
    /// Fixed discrete legal mask.
    pub action_mask: Vec<bool>,
    /// Seats permitted to act next.
    pub to_act: Vec<PlayerId>,
    /// Current per-seat returns.
    pub returns: Vec<f64>,
    /// This operation's per-seat reward deltas; zero for non-step operations.
    pub rewards: Vec<f64>,
    /// Rules-based ending.
    pub terminated: bool,
    /// Cap-based ending.
    pub truncated: bool,
}

/// A successful checkpoint belongs exclusively to the training caller, not a player.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TrainingResult {
    /// Exact continuation and a separately scoped policy frame.
    Ok {
        /// Full private state/RNG for the owner; never forward to a policy.
        checkpoint: EnvSnapshot,
        /// Seat-scoped policy input and episode metadata.
        frame: TrainingFrame,
    },
    /// This operation failed; other rows remain independent.
    Error {
        /// Structured domain error.
        error: ApiError,
    },
}

/// Ordered independent results; there are no server-side environment handles.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TrainingBatchResult {
    /// One result per input, in input order.
    pub results: Vec<TrainingResult>,
}

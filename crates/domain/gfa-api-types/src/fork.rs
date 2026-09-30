//! Durable variations of existing matches.
use crate::{include_info_by_default, MatchState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Create a variation from a parent's local turn.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkMatch {
    /// Optional replacement assignments; omitted values retain the parent's seats.
    #[serde(default)]
    pub seats: Option<Vec<crate::Seat>>,
    /// Accepted-action count in the parent, including zero.
    pub turn: u64,
    /// Preserve the source RNG; requires full-state access for hidden-information games.
    #[serde(default)]
    pub keep_rng: bool,
    /// Fresh future-randomness seed; cannot be combined with keep_rng.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Include the new match's compact briefing.
    #[serde(default = "include_info_by_default")]
    pub include_info: bool,
}

/// Immutable reference to a fork point.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ForkSource {
    /// Parent match identifier.
    pub match_id: String,
    /// Local turn within the parent.
    pub turn: u64,
}

/// Public parent history through one fork point, ordered root first in a replay.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayAncestor {
    /// Parent and fork turn.
    pub source: ForkSource,
    /// Version used to rebuild this segment.
    pub engine_version: String,
    /// Spectator projections; private parent information requires separate authorization.
    pub states: Vec<MatchState>,
}

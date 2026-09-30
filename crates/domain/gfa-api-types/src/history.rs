//! Public match metadata and bounded history queries.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Current lifecycle status derived from the event log.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    /// The engine is waiting for another move.
    Active,
    /// The game reached a terminal result or a length cap.
    Finished,
}

/// Public metadata, excluding private state, seeds and player reasoning.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MatchMetadata {
    /// Match identifier.
    pub match_id: String,
    /// Registered game identifier.
    pub game_id: String,
    /// Engine version used for reconstruction.
    pub engine_version: String,
    /// Current lifecycle status.
    pub status: MatchStatus,
    /// Accepted action count.
    pub turn: u64,
    /// Seats currently required to act.
    pub to_act: Vec<u8>,
    /// Per-seat returns.
    pub returns: Vec<f64>,
    /// Rules ended play.
    pub terminated: bool,
    /// A length or external cap ended play.
    pub truncated: bool,
    /// Explicit control ending, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<crate::MatchOutcome>,
    /// Seat with a pending draw offer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_offer: Option<u8>,
    /// Creation timestamp in Unix milliseconds.
    pub created_at_ms: u64,
}

fn default_limit() -> u32 {
    50
}

/// Cursor query for local match history.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchHistoryQuery {
    /// Exclusive match-id cursor; omit for the first page.
    pub after: Option<String>,
    /// Restrict results to one registered game.
    pub game_id: Option<String>,
    /// Restrict results to active or finished games.
    pub status: Option<MatchStatus>,
    /// Maximum records scanned before filtering, from 1 to 100.
    #[serde(default = "default_limit")]
    pub limit: u32,
}

impl Default for MatchHistoryQuery {
    fn default() -> Self {
        Self {
            after: None,
            game_id: None,
            status: None,
            limit: default_limit(),
        }
    }
}

/// One bounded page; filters can produce an empty page with a continuation cursor.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MatchHistory {
    /// Matching records from this page, in ascending match-id order.
    pub matches: Vec<MatchMetadata>,
    /// Exclusive cursor for the next page; null means the scan is exhausted.
    pub next: Option<String>,
}

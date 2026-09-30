//! Paginated event history projected to an authorized viewer.
use crate::{Assists, ForkSource, MatchState, Seat};
use gfa_core::{GameEvent, LegalAction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_limit() -> u32 { 50 }

/// Cursor and view requested for one event-log page.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsQuery {
    /// Exclusive event sequence; omit to start at sequence zero.
    pub since: Option<u64>,
    /// Page size, 1..100.
    #[serde(default = "default_limit")]
    #[cfg_attr(feature = "openapi", schema(minimum = 1, maximum = 100))]
    pub limit: u32,
    /// Requested player; omit for the public spectator view.
    pub seat: Option<u8>,
}

impl Default for EventsQuery {
    fn default() -> Self { Self { since:None, limit:50, seat:None } }
}

/// A public or seat-scoped event payload. Storage records are never serialized directly.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventData {
    /// Canonical initial inputs and this viewer's starting observation.
    Created {
        /// Registered game.
        game_id: String,
        /// Recorded rules version.
        engine_version: String,
        /// Public game options.
        config: Value,
        /// RNG seed, disclosed only to an explicitly authorized omniscient caller.
        #[serde(skip_serializing_if = "Option::is_none")]
        seed: Option<u64>,
        /// Player assignments.
        seats: Vec<Seat>,
        /// Recorded assistance policy.
        assists: Assists,
        /// Pristine initial state, before any same-turn controls.
        initial: Box<MatchState>,
        /// Recorded Unix creation time.
        at_ms: u64,
    },
    /// Accepted action, with private details omitted for other viewers.
    Action {
        /// Turn before the action.
        turn: u64,
        /// Acting seat.
        seat: u8,
        /// Visible action, absent for another player's hidden move.
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<LegalAction>,
        /// Own reasoning; other reasoning becomes visible only after a perfect-information game ends.
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
        /// Provider diagnostics under the same disclosure rule as reasoning.
        #[serde(skip_serializing_if = "Option::is_none")]
        opponent_info: Option<Value>,
        /// Only engine events visible to this viewer.
        events: Vec<GameEvent>,
        /// Recorded Unix acceptance time.
        at_ms: u64,
    },
    /// Public branch point; internal source permissions and revisions stay private.
    ForkedFrom {
        /// Parent reference.
        source: ForkSource,
    },
    /// A seat resigned.
    Resigned {
        /// Turn at the control.
        turn: u64,
        /// Resigning seat.
        seat: u8,
        /// Recorded Unix time.
        at_ms: u64,
    },
    /// A draw was offered or accepted.
    DrawOffered {
        /// Turn at the control.
        turn: u64,
        /// Acting seat.
        seat: u8,
        /// Recorded Unix time.
        at_ms: u64,
    },
    /// Engine completion or truncation.
    Finished {
        /// Final accepted-action count.
        turn: u64,
        /// Final per-seat returns.
        returns: Vec<f64>,
        /// Rules ended play.
        terminated: bool,
        /// A cap ended play.
        truncated: bool,
    },
}

/// An event's stable sequence and projected payload.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecordedEvent {
    /// Zero-based sequence in this match's durable log.
    pub sequence: u64,
    /// Viewer-scoped contents.
    #[serde(flatten)]
    pub event: EventData,
}

/// A bounded page from one consistent revision.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventPage {
    /// Match identifier.
    pub match_id: String,
    /// Event count in the snapshot used for this page.
    pub revision: u64,
    /// Consecutive projected events in ascending sequence.
    pub events: Vec<RecordedEvent>,
    /// Exclusive cursor when this snapshot has more events.
    pub next: Option<u64>,
}

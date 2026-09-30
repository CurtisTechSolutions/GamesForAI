use gfa_api_types::{MoveRequest, MoveResult, Start};
use gfa_core::{LegalAction, PlayerId, StepEvents};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Immutable engine inputs, recorded as the first event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchOrigin {
    /// Registered game id.
    pub game_id: String,
    /// Engine version required for exact replay.
    pub engine_version: String,
    /// Game configuration.
    pub config: Value,
    /// Seed actually used by the engine.
    pub seed: u64,
    /// Optional validated custom start.
    pub start: Option<Start>,
    /// Assistance policy; old records use the local-play defaults.
    #[serde(default)]
    pub assists: gfa_api_types::Assists,
    /// Unix milliseconds from the injected clock.
    pub created_at_ms: u64,
    /// Internal fork flag; preserve a complete snapshot's serialized RNG.
    #[serde(default)]
    pub preserve_start_rng: bool,
    /// Internal benchmark assignment; active benchmark matches cannot be forked.
    #[serde(default)]
    pub benchmark_run: Option<String>,
}

/// One action accepted by the engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppliedAction {
    /// Turn before the action.
    pub turn: u64,
    /// Acting seat.
    pub seat: PlayerId,
    /// All canonical action encodings.
    pub action: LegalAction,
    /// Optional player-supplied explanation.
    pub reasoning: Option<String>,
    /// Engine transition events, retained privately in the store.
    pub engine_events: StepEvents,
    /// Time from the injected clock.
    pub accepted_at_ms: u64,
}

/// Append-only facts from which the match can be reconstructed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum MatchEvent {
    /// Optional variation preface, followed immediately by MatchCreated.
    ForkedFrom {
        /// Parent match and local turn.
        source: gfa_api_types::ForkSource,
        /// View preserved when sampling the child.
        viewer: gfa_core::Viewer,
        /// Event revision of the parent snapshot; later controls cannot rewrite history.
        parent_revision: u64,
    },
    /// Initial game inputs; exactly one, at sequence zero.
    MatchCreated(MatchOrigin),
    /// Validated action.
    Action(AppliedAction),
    /// A resignation ends a one- or two-seat match without changing its board.
    Resigned {
        /// Accepted action count when the control was submitted.
        turn: u64,
        /// Resigning seat.
        seat: PlayerId,
        /// Unix milliseconds.
        at_ms: u64,
    },
    /// Offer a draw, or accept the other seat's current offer.
    DrawOffered {
        /// Accepted action count when the control was submitted.
        turn: u64,
        /// Offering or accepting seat.
        seat: PlayerId,
        /// Unix milliseconds.
        at_ms: u64,
    },
    /// Ending marker, checked against the replayed engine result.
    MatchFinished {
        /// Accepted action count at completion.
        turn: u64,
        /// Final per-seat returns.
        returns: Vec<f64>,
        /// Whether rules ended play.
        terminated: bool,
        /// Whether a cap ended play.
        truncated: bool,
    },
}

/// An idempotent command, persisted atomically with its events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredCommand {
    /// Opaque caller-provided key, scoped to this match.
    pub key: String,
    /// Original request, used to reject key reuse with different input.
    pub request: MoveRequest,
    /// Original response, returned exactly on a retry.
    pub response: MoveResult,
}

/// An atomic storage snapshot; events are ordered by contiguous sequence number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchRecord {
    /// Stable identifier.
    pub id: String,
    /// Complete ordered source of truth.
    pub events: Vec<MatchEvent>,
    /// Successful commands retained for retry deduplication.
    pub commands: Vec<StoredCommand>,
}

/// Storage append result. Duplicate commands do not append any events.
#[derive(Clone, Debug, PartialEq)]
pub enum AppendResult {
    /// Events and optional command were committed atomically.
    Appended,
    /// The same command was already committed, including during a race.
    AlreadyCommitted(Box<MoveResult>),
}

impl MatchRecord {
    pub(crate) fn origin(&self) -> Option<&MatchOrigin> {
        match self.events.as_slice() {
            [MatchEvent::MatchCreated(origin), ..]
            | [MatchEvent::ForkedFrom { .. }, MatchEvent::MatchCreated(origin), ..] => Some(origin),
            _ => None,
        }
    }

    pub(crate) fn fork_source(&self) -> Option<&gfa_api_types::ForkSource> {
        match self.events.first() {
            Some(MatchEvent::ForkedFrom { source, .. }) => Some(source),
            _ => None,
        }
    }
}

/// Trusted host authorization for a fork; transport input must not set these flags.
#[derive(Clone, Copy, Debug)]
pub struct ForkAccess {
    /// Already-authorized source viewer.
    pub viewer: gfa_core::Viewer,
    /// The caller owns the parent; required while it is active.
    pub owns_parent: bool,
    /// The caller may inspect the complete state, including hidden cards and RNG.
    pub full_state: bool,
}

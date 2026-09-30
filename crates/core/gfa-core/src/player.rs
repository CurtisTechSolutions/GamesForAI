//! Observation-only player contracts shared by in-process and external adapters.
use crate::{ErrorCode, GameError, LegalAction, Observation, PlayerId};
use serde::{Deserialize, Serialize};

/// Monotonic time supplied by the host, independently of game state.
pub trait Clock: Send + Sync {
    /// Milliseconds since an arbitrary fixed origin.
    fn now_ms(&self) -> u64;
}

/// All information supplied to a player for one decision.
pub struct PlayerTurn<'a> {
    /// Acting seat.
    pub seat: PlayerId,
    /// Seat-scoped observation; never a complete live match state.
    pub observation: &'a Observation,
    /// Canonical legal actions for this seat.
    pub legal_actions: &'a [LegalAction],
}

/// Hard search limits. Fixed seed and node budgets reproduce decisions.
#[derive(Clone, Copy, Debug)]
pub struct SearchLimits {
    /// Maximum visited nodes, including rollout steps.
    pub nodes: u64,
    /// Maximum minimax depth and MCTS tree/rollout length.
    pub depth: u8,
    /// Wall clock time allowed, checked before each node.
    pub time_ms: u64,
    /// Independent planning RNG seed.
    pub seed: u64,
}

impl SearchLimits {
    /// Provisional resource ladder. Strength calibration is reported separately.
    pub fn for_level(level: u8, seed: u64) -> Result<Self, GameError> {
        if !(1..=10).contains(&level) {
            return Err(invalid("Level must be 1..10"));
        }
        Ok(Self {
            nodes: 128_u64 << level,
            depth: level,
            time_ms: 5_000,
            seed,
        })
    }

    /// Reject zero or excessive resource budgets before scheduling work.
    pub fn validate(self) -> Result<Self, GameError> {
        if self.nodes == 0
            || self.nodes > 1_000_000
            || self.depth == 0
            || self.depth > 64
            || self.time_ms == 0
            || self.time_ms > 60_000
        {
            return Err(invalid(
                "Search requires 1..1000000 nodes, 1..64 depth and 1..60000 ms",
            ));
        }
        Ok(self)
    }
}

/// Search diagnostics accompanying a chosen move.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]

pub struct ChoiceInfo {
    /// Optional engine-provided solving explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advice: Option<crate::AdviceInfo>,
    /// Algorithm identifier.
    pub algorithm: String,
    /// Number of visited nodes.
    pub nodes: u64,
    /// Deepest completed minimax iteration or deepest MCTS path.
    pub depth: u8,
    /// Root player's expected return, absent without completed search.
    pub evaluation: Option<f64>,
    /// Canonical principal variation, beginning with the chosen move.
    pub principal_variation: Vec<String>,
    /// Whether node or time limits interrupted searching.
    pub budget_exhausted: bool,
}

/// One legal move and optional explanatory diagnostics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]

pub struct ActionChoice {
    /// Canonical action selected from the supplied legal list.
    pub action: LegalAction,
    /// Search diagnostics.
    pub info: ChoiceInfo,
}

/// A player that receives only its observation, legal actions and search clock.
pub trait Opponent: Send + Sync {
    /// Choose one legal action without modifying the match.
    fn choose_action(
        &self,
        turn: &PlayerTurn<'_>,
        limits: SearchLimits,
        clock: &dyn Clock,
    ) -> Result<ActionChoice, GameError>;
}


fn invalid(message: &str) -> GameError {
    GameError::new(
        ErrorCode::InvalidConfig,
        message,
        "Use a supported opponent and bounded limits.",
    )
}

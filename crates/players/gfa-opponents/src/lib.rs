//! Bounded, observation-only in-process opponents.
mod mcts;
mod minimax;

use gfa_core::serde::{Deserialize, Serialize};
use gfa_core::serde_json::Value;
use gfa_core::{
    DynGame, ErrorCode, GameError, Information, LegalAction, Observation, PlayerId, SeededRng,
    TurnStructure, Viewer,
};
use std::{sync::Arc, time::Instant};

/// Monotonic time supplied by the host, independently of game state.
pub trait Clock: Send + Sync {
    /// Milliseconds since an arbitrary fixed origin.
    fn now_ms(&self) -> u64;
}

/// System monotonic clock for interactive play.
pub struct SystemClock(Instant);

impl Default for SystemClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
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
#[serde(crate = "gfa_core::serde")]
pub struct ChoiceInfo {
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
#[serde(crate = "gfa_core::serde")]
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

/// Uniform, reproducible random baseline for any game.
pub struct Random;

impl Opponent for Random {
    fn choose_action(
        &self,
        turn: &PlayerTurn<'_>,
        limits: SearchLimits,
        _: &dyn Clock,
    ) -> Result<ActionChoice, GameError> {
        let limits = limits.validate()?;
        let index = SeededRng::new(limits.seed)
            .index(turn.legal_actions.len())
            .ok_or_else(|| GameError::illegal("No legal actions for this seat"))?;
        Ok(choice(turn.legal_actions[index].clone(), "random"))
    }
}

/// In-process tree search algorithm.
#[derive(Clone, Copy, Debug)]
pub enum Algorithm {
    /// Iterative deepening alpha-beta minimax for two-player zero-sum games.
    Minimax,
    /// UCT with random rollouts and per-seat return backup.
    Mcts,
}

/// Search over an injected engine, reconstructing state exclusively from observations.
pub struct SearchOpponent {
    game: Arc<dyn DynGame>,
    config: Value,
    algorithm: Algorithm,
}

impl SearchOpponent {
    /// Build a search player for a deterministic perfect-information sequential game.
    ///
    /// Stochastic and hidden-information search require dedicated sampling policies.
    /// Refusing them here prevents accidental use of hidden live state or future RNG.
    pub fn new(
        game: Arc<dyn DynGame>,
        config: Value,
        algorithm: Algorithm,
    ) -> Result<Self, GameError> {
        let spec = game.spec();
        if spec.information != Information::Perfect
            || spec.stochastic
            || spec.turn_structure != TurnStructure::Sequential
            || (matches!(algorithm, Algorithm::Minimax) && spec.num_players != [2, 2])
        {
            return Err(invalid(
                "Search requires a supported sequential perfect-information game",
            ));
        }
        if !spec.reward_range.iter().all(|v| v.is_finite())
            || spec.reward_range[0] >= spec.reward_range[1]
        {
            return Err(invalid("Search requires a finite increasing reward range"));
        }
        let config = game.normalize_config(&config)?;
        game.initial_state(&config, 0)?;
        Ok(Self {
            game,
            config,
            algorithm,
        })
    }
}

impl Opponent for SearchOpponent {
    fn choose_action(
        &self,
        turn: &PlayerTurn<'_>,
        limits: SearchLimits,
        clock: &dyn Clock,
    ) -> Result<ActionChoice, GameError> {
        let limits = limits.validate()?;
        let mut budget = Budget::new(limits, clock);
        let state = self.game.state_from_observation(
            &self.config,
            turn.observation,
            Viewer::Player(turn.seat),
            limits.seed,
        )?;
        if self.game.is_terminal(&state)?
            || self.game.current_players(&state)? != [turn.seat]
            || self.game.legal_actions(&state, turn.seat)? != turn.legal_actions
        {
            return Err(GameError::illegal(
                "Observation and legal actions do not describe this turn",
            ));
        }
        match self.algorithm {
            Algorithm::Minimax => minimax::choose(self.game.as_ref(), &state, turn, &mut budget),
            Algorithm::Mcts => mcts::choose(self.game.as_ref(), &state, turn, &mut budget),
        }
    }
}

fn invalid(message: &str) -> GameError {
    GameError::new(
        ErrorCode::InvalidConfig,
        message,
        "Use a supported opponent and bounded limits.",
    )
}

fn choice(action: LegalAction, algorithm: &str) -> ActionChoice {
    ActionChoice {
        info: ChoiceInfo {
            algorithm: algorithm.into(),
            nodes: 0,
            depth: 0,
            evaluation: None,
            principal_variation: vec![action.string.clone()],
            budget_exhausted: false,
        },
        action,
    }
}

struct Budget<'a> {
    limits: SearchLimits,
    clock: &'a dyn Clock,
    deadline: u64,
    nodes: u64,
    exhausted: bool,
}

impl<'a> Budget<'a> {
    fn new(limits: SearchLimits, clock: &'a dyn Clock) -> Self {
        Self {
            limits,
            clock,
            deadline: clock.now_ms().saturating_add(limits.time_ms),
            nodes: 0,
            exhausted: false,
        }
    }

    fn enter(&mut self) -> bool {
        if self.nodes >= self.limits.nodes || self.clock.now_ms() >= self.deadline {
            self.exhausted = true;
            false
        } else {
            self.nodes += 1;
            true
        }
    }
}

fn actor(game: &dyn DynGame, state: &Value) -> Result<PlayerId, GameError> {
    let players = game.current_players(state)?;
    if players.len() != 1 {
        return Err(GameError::illegal(
            "Search expected exactly one current player",
        ));
    }
    Ok(players[0])
}

fn payoff(game: &dyn DynGame, state: &Value, seat: PlayerId) -> Result<f64, GameError> {
    let returns = game.returns(state)?;
    let value = returns
        .get(usize::from(seat))
        .copied()
        .ok_or_else(|| GameError::illegal("Engine omitted a player's return"))?;
    if !value.is_finite() {
        return Err(GameError::illegal("Engine produced a non-finite return"));
    }
    Ok(value)
}

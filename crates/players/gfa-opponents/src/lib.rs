//! Bounded, observation-only in-process opponents.
mod mcts;
mod minimax;
mod reference;
pub use reference::ReferenceOpponent;
// Re-export the original paths for downstream source compatibility.
pub use gfa_core::{ActionChoice, ChoiceInfo, Clock, Opponent, PlayerTurn, SearchLimits};

use gfa_core::serde_json::Value;
use gfa_core::{
    DynGame, ErrorCode, GameError, Information, LegalAction, PlayerId, SeededRng, TurnStructure,
    Viewer,
};
use std::{sync::Arc, time::Instant};

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
            advice: None,
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

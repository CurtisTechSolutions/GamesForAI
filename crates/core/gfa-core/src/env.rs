use crate::{ErrorCode, Game, GameError, Observation, PlayerId, StepEvents, Viewer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// A caller-owned native environment. No network, database, or JSON step boundary.
/// Full state access is for trusted training code; observations remain seat-scoped.
pub struct Env<G: Game> {
    config: Arc<G::Config>,
    state: G::State,
    action_space_size: u32,
}
impl<G: Game> Clone for Env<G> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state.clone(),
            action_space_size: self.action_space_size,
        }
    }
}

/// Versioned full-state checkpoint, including private information and engine RNG.
/// Never return this object through a player's observation or a public API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvSnapshot {
    /// Snapshot format version.
    pub format_version: u8,
    /// Stable game identifier.
    pub game_id: String,
    /// Engine version needed for compatible replay.
    pub engine_version: String,
    /// Exact normalized environment configuration.
    pub config: Value,
    /// Complete engine state.
    pub state: Value,
}

/// One transition. Rewards are deltas of per-seat engine returns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvStep {
    /// Immediate reward for every seat, in seat order.
    pub rewards: Vec<f64>,
    /// Rules-based episode ending.
    pub terminated: bool,
    /// Length/time cap ending, distinct from termination.
    pub truncated: bool,
    /// Caller-owned engine events, including private scopes.
    pub events: StepEvents,
}
impl<G: Game> Env<G> {
    /// Create a seeded environment with the game's default configuration.
    pub fn new(seed: u64) -> Result<Self, GameError> {
        Self::with_config(G::Config::default(), seed)
    }

    /// Create a seeded environment with explicit options.
    pub fn with_config(config: G::Config, seed: u64) -> Result<Self, GameError> {
        let state = G::new_initial_state(&config, seed)?;
        G::validate_state(&state)?;
        Ok(Self {
            config: Arc::new(config),
            state,
            action_space_size: G::spec().action_space_size,
        })
    }

    /// Reset to a fresh seed or a validated position. Failure leaves the old state.
    /// Imported positions use the requested seed for future stochastic transitions.
    pub fn reset(&mut self, seed: u64, position: Option<&str>) -> Result<(), GameError> {
        let state = if let Some(position) = position {
            let mut state = G::state_from_notation(&self.config, position)?;
            G::reseed(&mut state, seed)?;
            state
        } else {
            G::new_initial_state(&self.config, seed)?
        };
        G::validate_state(&state)?;
        self.state = state;
        Ok(())
    }

    /// Seats permitted to act now; empty after an episode ends.
    pub fn current_players(&self) -> Vec<PlayerId> {
        G::current_players(&self.state)
    }

    /// Current rules ending.
    pub fn terminated(&self) -> bool {
        G::is_terminal(&self.state)
    }

    /// Current cap ending, recoverable after state restoration.
    pub fn truncated(&self) -> bool {
        G::is_truncated(&self.state)
    }

    /// Project only the requested information set.
    pub fn observe(&self, viewer: Viewer) -> Result<Observation, GameError> {
        if let Viewer::Player(seat) = viewer {
            self.validate_seat(seat)?;
        }
        Ok(G::observe(&self.state, viewer))
    }

    /// Legal typed actions for the selected seat; empty when that seat cannot act.
    pub fn legal_actions(&self, player: PlayerId) -> Result<Vec<G::Action>, GameError> {
        self.validate_seat(player)?;
        Ok(G::legal_actions(&self.state, player))
    }

    /// Fixed discrete mask suitable for action-masked neural policies.
    pub fn action_mask(&self, player: PlayerId) -> Result<Vec<bool>, GameError> {
        let mut mask = vec![false; self.action_space_size as usize];
        for action in self.legal_actions(player)? {
            let slot = mask.get_mut(G::action_to_index(&action) as usize)
                .ok_or_else(|| GameError::illegal("Engine action index exceeds its declared space"))?;
            *slot = true;
        }
        Ok(mask)
    }

    /// Apply one typed action. The engine's atomic transition contract is preserved.
    pub fn step(&mut self, player: PlayerId, action: &G::Action) -> Result<EnvStep, GameError> {
        if self.terminated() || self.truncated() {
            return Err(GameError::new(
                ErrorCode::MatchFinished,
                "The training episode has ended",
                "Reset the environment before stepping again.",
            ));
        }
        self.validate_seat(player)?;
        let before = G::returns(&self.state);
        let events = G::apply(&mut self.state, player, action)?;
        let after = G::returns(&self.state);
        // Games guarantee finite returns in stable seat order.
        let rewards = after.iter().zip(before).map(|(after, before)| after - before).collect();
        Ok(EnvStep {
            rewards,
            terminated: self.terminated(),
            truncated: events.truncated || self.truncated(),
            events,
        })
    }

    /// Decode and apply a discrete action index.
    pub fn step_index(&mut self, player: PlayerId, index: u32) -> Result<EnvStep, GameError> {
        let action = G::action_from_index(&self.state, index)?;
        self.step(player, &action)
    }

    /// Decode and apply a canonical action string.
    pub fn step_string(&mut self, player: PlayerId, action: &str) -> Result<EnvStep, GameError> {
        let action = G::action_from_string(&self.state, action)?;
        self.step(player, &action)
    }

    /// Inspect caller-owned full state for trusted search or simulation.
    pub fn state(&self) -> &G::State {
        &self.state
    }

    /// Export an exact checkpoint, preserving hidden state and RNG.
    pub fn get_state(&self) -> Result<EnvSnapshot, GameError> {
        let spec = G::spec();
        Ok(EnvSnapshot {
            format_version: 1,
            game_id: spec.id,
            engine_version: spec.engine_version,
            config: serde_json::to_value(self.config.as_ref())?,
            state: serde_json::to_value(&self.state)?,
        })
    }

    /// Validate an exact checkpoint before atomically replacing state.
    /// A different game, version, or configuration is rejected.
    pub fn set_state(&mut self, snapshot: &EnvSnapshot) -> Result<(), GameError> {
        let spec = G::spec();
        if snapshot.format_version != 1
            || snapshot.game_id != spec.id
            || snapshot.engine_version != spec.engine_version
            || snapshot.config != serde_json::to_value(self.config.as_ref())?
        {
            return Err(GameError::position("Training checkpoint does not match this environment"));
        }
        let state: G::State = serde_json::from_value(snapshot.state.clone())?;
        G::validate_state(&state)?;
        // Check the embedded state configuration against this environment.
        let notation = G::state_to_notation(&state)?;
        let restored = G::state_from_notation(&self.config, &notation)?;
        if serde_json::to_value(&restored)? != snapshot.state {
            return Err(GameError::position("Checkpoint cannot be restored exactly"));
        }
        self.state = restored;
        Ok(())
    }

    fn validate_seat(&self, player: PlayerId) -> Result<(), GameError> {
        if usize::from(player) >= G::returns(&self.state).len() {
            return Err(GameError::position("Training seat is out of range"));
        }
        Ok(())
    }
}

use crate::{choice, invalid, ActionChoice, Clock, Opponent, PlayerTurn, SearchLimits};
use gfa_core::{serde_json::Value, DynGame, GameError, Viewer};
use std::sync::Arc;

/// Game-provided reference solver, invoked only on an observation reconstruction.
pub struct ReferenceOpponent {
    game: Arc<dyn DynGame>,
    config: Value,
}

impl ReferenceOpponent {
    /// Require the engine's declared reference capability.
    pub fn new(game: Arc<dyn DynGame>, config: Value) -> Result<Self, GameError> {
        if !game.supports_reference_advice() {
            return Err(invalid("This game has no reference solver"));
        }
        let config = game.normalize_config(&config)?;
        Ok(Self { game, config })
    }
}

impl Opponent for ReferenceOpponent {
    fn choose_action(
        &self,
        turn: &PlayerTurn<'_>,
        limits: SearchLimits,
        _: &dyn Clock,
    ) -> Result<ActionChoice, GameError> {
        let limits = limits.validate()?;
        let state = self.game.state_from_observation(
            &self.config,
            turn.observation,
            Viewer::Player(turn.seat),
            limits.seed,
        )?;
        if self.game.legal_actions(&state, turn.seat)? != turn.legal_actions {
            return Err(GameError::illegal("Observation and legal actions disagree"));
        }
        let advice = self
            .game
            .reference_advice(&state, turn.seat)?
            .ok_or_else(|| GameError::illegal("No reference action is available"))?;
        if !turn.legal_actions.contains(&advice.action) {
            return Err(GameError::illegal(
                "Reference solver returned an illegal action",
            ));
        }
        let mut result = choice(advice.action, "reference");
        result.info.advice = Some(advice.info);
        Ok(result)
    }
}

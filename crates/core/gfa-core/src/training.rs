use crate::{
    Env, EnvSnapshot, EnvStep, Game, GameError, GameSpec, LegalAction, Observation, PlayerId,
    Viewer,
};

/// Object-safe native environments for Python bindings and parallel batches.
/// Implementations retain typed engine state between steps.
pub trait TrainingEnv: Send + Sync {
    /// Clone caller-owned state for independent search.
    fn clone_box(&self) -> Box<dyn TrainingEnv>;
    /// Static metadata; hosts should cache this outside their hot loop.
    fn spec(&self) -> GameSpec;
    /// Reset to a seed or a validated curriculum position.
    fn reset(&mut self, seed: u64, position: Option<&str>) -> Result<(), GameError>;
    /// Seats permitted to act.
    fn current_players(&self) -> Vec<PlayerId>;
    /// Rules-based ending.
    fn terminated(&self) -> bool;
    /// Cap-based ending.
    fn truncated(&self) -> bool;
    /// Per-seat cumulative returns.
    fn returns(&self) -> Vec<f64>;
    /// Information-set projection.
    fn observe(&self, viewer: Viewer) -> Result<Observation, GameError>;
    /// Optional optimized numeric projection, preserving the viewer's scope.
    fn observe_tensor(&self, viewer: Viewer) -> Result<Option<crate::Tensor>, GameError> {
        Ok(self.observe(viewer)?.tensor)
    }
    /// Fixed discrete action mask.
    fn action_mask(&self, player: PlayerId) -> Result<Vec<bool>, GameError>;
    /// Optional readable actions for text policies, scoped to the selected seat.
    fn action_catalog(&self, _player: PlayerId) -> Result<Vec<LegalAction>, GameError> {
        Err(GameError::position(
            "This environment has no readable action catalog",
        ))
    }
    /// Native discrete action step.
    fn step_index(&mut self, player: PlayerId, index: u32) -> Result<EnvStep, GameError>;
    /// Canonical notation step.
    fn step_string(&mut self, player: PlayerId, action: &str) -> Result<EnvStep, GameError>;
    /// Trusted full-state checkpoint.
    fn get_state(&self) -> Result<EnvSnapshot, GameError>;
    /// Validate and restore a compatible full-state checkpoint.
    fn set_state(&mut self, snapshot: &EnvSnapshot) -> Result<(), GameError>;
}
impl Clone for Box<dyn TrainingEnv> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
impl<G: Game> TrainingEnv for Env<G>
where
    G::Config: Send + Sync,
{
    fn clone_box(&self) -> Box<dyn TrainingEnv> {
        Box::new(self.clone())
    }
    fn spec(&self) -> GameSpec {
        G::spec()
    }
    fn reset(&mut self, seed: u64, position: Option<&str>) -> Result<(), GameError> {
        Env::reset(self, seed, position)
    }
    fn current_players(&self) -> Vec<PlayerId> {
        Env::current_players(self)
    }
    fn terminated(&self) -> bool {
        Env::terminated(self)
    }
    fn truncated(&self) -> bool {
        Env::truncated(self)
    }
    fn returns(&self) -> Vec<f64> {
        G::returns(self.state())
    }
    fn observe(&self, viewer: Viewer) -> Result<Observation, GameError> {
        Env::observe(self, viewer)
    }
    fn observe_tensor(&self, viewer: Viewer) -> Result<Option<crate::Tensor>, GameError> {
        Env::observe_tensor(self, viewer)
    }
    fn action_mask(&self, player: PlayerId) -> Result<Vec<bool>, GameError> {
        Env::action_mask(self, player)
    }
    fn action_catalog(&self, player: PlayerId) -> Result<Vec<LegalAction>, GameError> {
        let mut actions = self
            .legal_actions(player)?
            .iter()
            .map(|action| {
                Ok(LegalAction {
                    string: G::action_to_string(self.state(), action),
                    json: serde_json::to_value(action)?,
                    index: G::action_to_index(action),
                })
            })
            .collect::<Result<Vec<_>, GameError>>()?;
        actions.sort_by(|left, right| left.string.cmp(&right.string));
        Ok(actions)
    }
    fn step_index(&mut self, player: PlayerId, index: u32) -> Result<EnvStep, GameError> {
        Env::step_index(self, player, index)
    }
    fn step_string(&mut self, player: PlayerId, action: &str) -> Result<EnvStep, GameError> {
        Env::step_string(self, player, action)
    }
    fn get_state(&self) -> Result<EnvSnapshot, GameError> {
        Env::get_state(self)
    }
    fn set_state(&mut self, snapshot: &EnvSnapshot) -> Result<(), GameError> {
        Env::set_state(self, snapshot)
    }
}

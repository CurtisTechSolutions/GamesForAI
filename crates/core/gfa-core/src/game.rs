use crate::{GameError, GameSpec, Observation, PlayerId, StepEvents, Viewer};
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Serialize};
use std::hash::Hash;

/// Synchronous, pure game rules. Engines must not perform I/O or use global randomness.
pub trait Game: Send + Sync + 'static {
    /// Complete serializable state, including any RNG.
    type State: Clone + Serialize + DeserializeOwned + Send + Sync;
    /// A structured action with an injective index and canonical string representation.
    type Action: Clone + Serialize + DeserializeOwned + Eq + Hash + JsonSchema;
    /// Validated game options.
    type Config: Default + Serialize + DeserializeOwned + JsonSchema;

    /// Rules and generated schemas.
    fn spec() -> GameSpec;
    /// Explanations for model briefings. Registered games must supply a valid guide.
    ///
    /// The default preserves compatibility for existing in-process engines.
    fn play_guide() -> Option<crate::PlayGuide> {
        None
    }

    /// Create a state, rejecting unsupported configuration.
    fn new_initial_state(config: &Self::Config, seed: u64) -> Result<Self::State, GameError>;
    /// Replace future randomness after importing a position, without changing its board.
    ///
    /// Stochastic engines must override this. Deterministic engines need no RNG.
    fn reseed(state: &mut Self::State, _seed: u64) -> Result<(), GameError> {
        if Self::spec().stochastic {
            return Err(GameError::position(
                "This stochastic engine does not support reseeding",
            ));
        }
        Self::validate_state(state)
    }

    /// Validate state received through a persistence or API boundary.
    fn validate_state(state: &Self::State) -> Result<(), GameError>;
    /// Seats that can act; empty after a terminal or truncated transition.
    fn current_players(state: &Self::State) -> Vec<PlayerId>;
    /// Actions legal for this seat in this state.
    fn legal_actions(state: &Self::State, player: PlayerId) -> Vec<Self::Action>;
    /// Apply one action atomically. An error must never mutate the state.
    fn apply(
        state: &mut Self::State,
        player: PlayerId,
        action: &Self::Action,
    ) -> Result<StepEvents, GameError>;
    /// Whether the rules have ended play.
    fn is_terminal(state: &Self::State) -> bool;
    /// Per-seat returns in seat order.
    fn returns(state: &Self::State) -> Vec<f64>;
    /// Project state into the viewer's information set.
    fn observe(state: &Self::State, viewer: Viewer) -> Observation;
    /// Encode an action into canonical notation.
    fn action_to_string(state: &Self::State, action: &Self::Action) -> String;
    /// Parse canonical notation; legality is checked separately by apply.
    fn action_from_string(state: &Self::State, text: &str) -> Result<Self::Action, GameError>;
    /// Encode an action into a fixed discrete index.
    fn action_to_index(action: &Self::Action) -> u32;
    /// Decode a discrete index.
    fn action_from_index(state: &Self::State, index: u32) -> Result<Self::Action, GameError>;
    /// Export a validated position.
    fn state_to_notation(state: &Self::State) -> Result<String, GameError>;
    /// Import and validate a position.
    fn state_from_notation(config: &Self::Config, notation: &str)
        -> Result<Self::State, GameError>;
}

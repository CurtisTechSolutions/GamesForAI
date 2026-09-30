use crate::{
    ErrorCode, Game, GameError, GameSpec, LegalAction, Observation, PlayerId, StepEvents, Viewer,
};
use serde_json::Value;
use std::{collections::BTreeMap, marker::PhantomData, sync::Arc};

/// Object-safe interface used by services, transports, and players.
pub trait DynGame: Send + Sync {
    /// Game metadata and schemas.
    fn spec(&self) -> GameSpec;
    /// Explanations for model briefings, if supplied by the engine.
    fn play_guide(&self) -> Option<crate::PlayGuide> {
        None
    }
    /// Normalize defaults through the engine's config type.
    ///
    /// Engines with custom implementations should override this when defaults
    /// need materializing. Initial-state construction still validates the options.
    fn normalize_config(&self, config: &Value) -> Result<Value, GameError> {
        Ok(config.clone())
    }
    /// Create an initial state from validated configuration.
    fn initial_state(&self, config: &Value, seed: u64) -> Result<Value, GameError>;
    /// Replace an imported position's future randomness while preserving its board.
    fn reseed(&self, state: &Value, _seed: u64) -> Result<Value, GameError> {
        if self.spec().stochastic {
            return Err(GameError::position(
                "This stochastic engine does not support reseeding",
            ));
        }
        self.validate_state(state)?;
        Ok(state.clone())
    }
    /// Reconstruct or sample a state using only a viewer-scoped observation.
    fn state_from_observation(
        &self,
        _config: &Value,
        _observation: &Observation,
        _viewer: Viewer,
        _seed: u64,
    ) -> Result<Value, GameError> {
        Err(GameError::position(
            "This engine cannot reconstruct planning states",
        ))
    }
    /// Whether the engine exposes a reference solver through the generic hook.
    fn supports_reference_advice(&self) -> bool {
        false
    }
    /// Obtain a legal recommendation from an authorized planning state.
    fn reference_advice(
        &self,
        _state: &Value,
        _player: PlayerId,
    ) -> Result<Option<crate::Advice<LegalAction>>, GameError> {
        Ok(None)
    }
    /// Validate a JSON state before accepting it from a caller.
    fn validate_state(&self, state: &Value) -> Result<(), GameError>;
    /// Seats currently allowed to act.
    fn current_players(&self, state: &Value) -> Result<Vec<PlayerId>, GameError>;
    /// Legal actions in all encodings, sorted by canonical notation.
    fn legal_actions(&self, state: &Value, player: PlayerId)
        -> Result<Vec<LegalAction>, GameError>;
    /// Apply a string, structured action, or index action; return a new state atomically.
    fn apply(
        &self,
        state: &Value,
        player: PlayerId,
        action: &Value,
    ) -> Result<(Value, StepEvents), GameError>;
    /// Whether the rules have ended play.
    fn is_terminal(&self, state: &Value) -> Result<bool, GameError>;
    /// Per-player returns.
    fn returns(&self, state: &Value) -> Result<Vec<f64>, GameError>;
    /// Viewer-scoped state.
    fn observe(&self, state: &Value, viewer: Viewer) -> Result<Observation, GameError>;
    /// Engine-approved public notation; None falls back to the viewer's observation.
    fn public_position(&self, _state: &Value) -> Result<Option<String>, GameError> {
        Ok(None)
    }

    /// Export a validated position.
    fn state_to_notation(&self, state: &Value) -> Result<String, GameError>;
    /// Validate and import a position.
    fn state_from_notation(&self, config: &Value, notation: &str) -> Result<Value, GameError>;

    /// Fixed-size legal mask, indexed by each action's discrete index.
    fn action_mask(&self, state: &Value, player: PlayerId) -> Result<Vec<bool>, GameError> {
        let mut mask = vec![false; self.spec().action_space_size as usize];
        for action in self.legal_actions(state, player)? {
            let slot = mask.get_mut(action.index as usize).ok_or_else(|| {
                GameError::illegal("Engine returned an action outside its declared index space")
            })?;
            *slot = true;
        }
        Ok(mask)
    }
}

/// Erases a typed game without introducing any game-specific service behavior.
pub struct GameAdapter<G: Game>(PhantomData<G>);

impl<G: Game> Default for GameAdapter<G> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<G: Game> GameAdapter<G> {
    fn state(value: &Value) -> Result<G::State, GameError> {
        let state = serde_json::from_value(value.clone())
            .map_err(|e| GameError::position(format!("{e}")))?;
        G::validate_state(&state)?;
        Ok(state)
    }

    fn config(value: &Value) -> Result<G::Config, GameError> {
        if value.is_null() {
            return Ok(G::Config::default());
        }
        serde_json::from_value(value.clone()).map_err(|e| {
            GameError::new(
                ErrorCode::InvalidConfig,
                e.to_string(),
                "Use the fields and values in config_schema.",
            )
        })
    }

    fn action(state: &G::State, value: &Value) -> Result<G::Action, GameError> {
        if let Some(text) = value.as_str() {
            return G::action_from_string(state, text);
        }
        if let Some(object) = value.as_object() {
            if object.len() == 1 && object.contains_key("index") {
                let index = object["index"]
                    .as_u64()
                    .and_then(|i| u32::try_from(i).ok())
                    .ok_or_else(|| {
                        GameError::new(
                            ErrorCode::UnparseableAction,
                            "Index must be an unsigned 32-bit integer",
                            "Use an index from legal_actions.",
                        )
                    })?;
                return G::action_from_index(state, index);
            }
        }
        serde_json::from_value(value.clone()).map_err(|e| {
            GameError::new(
                ErrorCode::UnparseableAction,
                e.to_string(),
                "Use canonical notation, the action schema, or {\"index\": n}.",
            )
        })
    }
}

impl<G: Game> DynGame for GameAdapter<G> {
    fn spec(&self) -> GameSpec {
        G::spec()
    }

    fn play_guide(&self) -> Option<crate::PlayGuide> {
        G::play_guide()
    }

    fn normalize_config(&self, config: &Value) -> Result<Value, GameError> {
        Ok(serde_json::to_value(Self::config(config)?)?)
    }

    fn initial_state(&self, config: &Value, seed: u64) -> Result<Value, GameError> {
        let state = G::new_initial_state(&Self::config(config)?, seed)?;
        G::validate_state(&state)?;
        Ok(serde_json::to_value(state)?)
    }

    fn state_from_observation(
        &self,
        config: &Value,
        observation: &Observation,
        viewer: Viewer,
        seed: u64,
    ) -> Result<Value, GameError> {
        let state = G::state_from_observation(&Self::config(config)?, observation, viewer, seed)?;
        G::validate_state(&state)?;
        if G::observe(&state, viewer).json != observation.json {
            return Err(GameError::position(
                "Planning state changed the viewer's observation",
            ));
        }
        Ok(serde_json::to_value(state)?)
    }

    fn supports_reference_advice(&self) -> bool {
        G::supports_reference_advice()
    }

    fn reference_advice(&self, state: &Value, player: PlayerId) -> Result<Option<crate::Advice<LegalAction>>, GameError> {
        let state = Self::state(state)?;
        let Some(advice) = G::reference_advice(&state, player)? else { return Ok(None); };
        if !G::legal_actions(&state, player).contains(&advice.action) {
            return Err(GameError::illegal("Reference solver returned an illegal action"));
        }
        Ok(Some(crate::Advice {
            action: LegalAction {
                string: G::action_to_string(&state, &advice.action),
                index: G::action_to_index(&advice.action),
                json: serde_json::to_value(advice.action)?,
            },
            info: advice.info,
        }))
    }

    fn validate_state(&self, state: &Value) -> Result<(), GameError> {
        Self::state(state).map(|_| ())
    }

    fn reseed(&self, state: &Value, seed: u64) -> Result<Value, GameError> {
        let mut state = Self::state(state)?;
        G::reseed(&mut state, seed)?;
        G::validate_state(&state)?;
        Ok(serde_json::to_value(state)?)
    }

    fn current_players(&self, state: &Value) -> Result<Vec<PlayerId>, GameError> {
        Ok(G::current_players(&Self::state(state)?))
    }

    fn legal_actions(
        &self,
        state: &Value,
        player: PlayerId,
    ) -> Result<Vec<LegalAction>, GameError> {
        let state = Self::state(state)?;
        let mut actions = G::legal_actions(&state, player)
            .into_iter()
            .map(|action| {
                Ok(LegalAction {
                    string: G::action_to_string(&state, &action),
                    index: G::action_to_index(&action),
                    json: serde_json::to_value(action)?,
                })
            })
            .collect::<Result<Vec<_>, GameError>>()?;
        actions.sort_by(|a, b| a.string.cmp(&b.string));
        Ok(actions)
    }

    fn apply(
        &self,
        state: &Value,
        player: PlayerId,
        action: &Value,
    ) -> Result<(Value, StepEvents), GameError> {
        let mut state = Self::state(state)?;
        let action = Self::action(&state, action)?;
        let events = G::apply(&mut state, player, &action)?;
        G::validate_state(&state)?;
        Ok((serde_json::to_value(state)?, events))
    }

    fn is_terminal(&self, state: &Value) -> Result<bool, GameError> {
        Ok(G::is_terminal(&Self::state(state)?))
    }

    fn returns(&self, state: &Value) -> Result<Vec<f64>, GameError> {
        Ok(G::returns(&Self::state(state)?))
    }

    fn observe(&self, state: &Value, viewer: Viewer) -> Result<Observation, GameError> {
        if let Viewer::Player(seat) = viewer {
            if seat >= self.spec().num_players[1] {
                return Err(GameError::position("Viewer seat is out of range"));
            }
        }
        Ok(G::observe(&Self::state(state)?, viewer))
    }

    fn public_position(&self, state: &Value) -> Result<Option<String>, GameError> {
        G::public_position(&Self::state(state)?)
    }

    fn state_to_notation(&self, state: &Value) -> Result<String, GameError> {
        G::state_to_notation(&Self::state(state)?)
    }

    fn state_from_notation(&self, config: &Value, notation: &str) -> Result<Value, GameError> {
        let state = G::state_from_notation(&Self::config(config)?, notation)?;
        G::validate_state(&state)?;
        Ok(serde_json::to_value(state)?)
    }
}

/// Ordered, constructor-injected registry. Only gfa-games names concrete game types.
#[derive(Default, Clone)]
pub struct GameRegistry {
    games: BTreeMap<String, Arc<dyn DynGame>>,
}

impl GameRegistry {
    /// Register a game, rejecting duplicate identifiers.
    pub fn register<G: Game>(&mut self) -> Result<(), GameError> {
        let spec = G::spec();
        if self.games.contains_key(&spec.id) {
            return Err(GameError::new(
                ErrorCode::InvalidConfig,
                format!("Duplicate game id: {}", spec.id),
                "Register each game exactly once.",
            ));
        }
        self.games
            .insert(spec.id, Arc::new(GameAdapter::<G>::default()));
        Ok(())
    }

    /// Resolve a game by its stable id.
    pub fn get(&self, id: &str) -> Result<Arc<dyn DynGame>, GameError> {
        self.games.get(id).cloned().ok_or_else(|| {
            GameError::new(
                ErrorCode::UnknownGame,
                format!("Unknown game: {id}"),
                "Choose a game returned by list_games.",
            )
        })
    }

    /// Metadata in stable identifier order.
    pub fn specs(&self) -> Vec<GameSpec> {
        self.games.values().map(|game| game.spec()).collect()
    }
}

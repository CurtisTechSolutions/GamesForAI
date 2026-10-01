//! Pure, deterministic contracts shared by every GamesForAI engine.
mod dynamic;
mod env;
mod error;
mod game;
mod player;
mod rng;
mod training;
mod types;

pub use dynamic::{DynGame, GameAdapter, GameRegistry};
pub use env::{Env, EnvSnapshot, EnvStep};
pub use error::{ErrorCode, GameError};
pub use game::Game;
pub use player::{
    ActionChoice, ChoiceInfo, Clock, Opponent, OpponentError, PlayerTurn, SearchLimits,
};
pub use rng::SeededRng;
/// Serialization and schema derives for game plugins, without extra direct dependencies.
pub use schemars;
pub use serde;
pub use serde_json;
pub use training::TrainingEnv;
pub use types::*;

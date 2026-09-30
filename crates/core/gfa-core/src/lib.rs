//! Pure, deterministic contracts shared by every GamesForAI engine.
mod dynamic;
mod error;
mod game;
mod player;
mod rng;
mod types;

pub use dynamic::{DynGame, GameAdapter, GameRegistry};
pub use error::{ErrorCode, GameError};
pub use game::Game;
pub use player::{ActionChoice, ChoiceInfo, Clock, Opponent, PlayerTurn, SearchLimits};
pub use rng::SeededRng;
/// Serialization and schema derives for game plugins, without extra direct dependencies.
pub use schemars;
pub use serde;
pub use serde_json;
pub use types::*;

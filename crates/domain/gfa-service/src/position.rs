use crate::error;
use gfa_api_types::{ApiError, Start};
use gfa_core::DynGame;
use serde_json::Value;

/// Import only caller-supplied positions; never use this to export a live match.
pub(crate) fn import(
    game: &dyn DynGame,
    config: &Value,
    start: &Start,
    seed: u64,
) -> Result<Value, ApiError> {
    let state = match start {
        Start::Position { position } => game.state_from_notation(config, position).map_err(error::engine)?,
        Start::State { state } => {
            game.validate_state(state).map_err(error::engine)?;
            state.clone()
        }
    };
    game.reseed(&state, seed).map_err(error::engine)
}

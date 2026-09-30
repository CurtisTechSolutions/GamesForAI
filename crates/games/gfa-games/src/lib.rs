//! The single registration point for compiled game plugins.
use gfa_core::{GameError, GameRegistry};

/// Assemble the registry selected by Cargo features.
pub fn registry() -> Result<GameRegistry, GameError> {
    #[allow(unused_mut)]
    let mut registry = GameRegistry::default();
    #[cfg(feature = "tictactoe")]
    registry.register::<gfa_game_tictactoe::TicTacToe>()?;
    #[cfg(feature = "connect4")]
    registry.register::<gfa_game_connect4::Connect4>()?;
    Ok(registry)
}

#[cfg(all(test, feature = "tictactoe"))]
mod tests {
    use super::*;
    use gfa_core::{serde_json::json, ErrorCode, Viewer};

    #[test]
    fn dynamic_encodings_agree() -> Result<(), GameError> {
        let registry = registry()?;
        let game = registry.get("tictactoe")?;
        let state = game.initial_state(&json!({}), 42)?;
        let expected = game.apply(&state, 0, &json!("r2c2"))?.0;
        assert_eq!(
            expected,
            game.apply(&state, 0, &json!({"row":2,"col":2}))?.0
        );
        assert_eq!(expected, game.apply(&state, 0, &json!({"index":4}))?.0);
        assert_eq!(game.action_mask(&state, 0)?, vec![true; 9]);
        assert_eq!(game.action_mask(&state, 1)?, vec![false; 9]);
        assert!(game.observe(&state, Viewer::Player(2)).is_err());
        assert!(game.initial_state(&json!({"unsupported":true}), 0).is_err());
        let error = game.apply(&state, 0, &json!({"index":-1}));
        assert!(matches!(
            error,
            Err(GameError {
                code: ErrorCode::UnparseableAction,
                ..
            })
        ));
        Ok(())
    }
}

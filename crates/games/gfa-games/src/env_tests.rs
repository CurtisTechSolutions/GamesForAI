use gfa_core::{serde_json, Env, EnvSnapshot, ErrorCode, Game, GameError, Viewer};
use gfa_game_chess::{ChessGame, Config as ChessConfig};
use gfa_game_sudoku::{Config as SudokuConfig, Sudoku};
use gfa_game_tictactoe::TicTacToe;

#[test]
fn native_episode_masks_rewards_and_state_restore() -> Result<(), GameError> {
    let mut env = Env::<TicTacToe>::new(7)?;
    assert_eq!(env.action_mask(0)?, vec![true; 9]);
    assert_eq!(env.action_mask(1)?, vec![false; 9]);
    assert!(env.observe(Viewer::Player(2)).is_err());
    env.step_string(0, "r1c1")?;
    let saved = env.get_state()?;
    let mut clone = env.clone();
    clone.step_string(1, "r3c3")?;
    assert_ne!(clone.get_state()?, env.get_state()?);
    let error = env
        .step_string(1, "r1c1")
        .err()
        .ok_or_else(|| GameError::illegal("expected rejection"))?;
    assert_eq!(error.code, ErrorCode::IllegalAction);
    assert_eq!(env.get_state()?, saved);
    for _ in 0..2 {
        env.set_state(&saved)?;
        env.step_index(1, 3)?;
        env.step_index(0, 1)?;
        env.step_index(1, 4)?;
        let finish = env.step_index(0, 2)?;
        assert_eq!(finish.rewards, vec![1.0, -1.0]);
        assert!(finish.terminated);
        assert!(!finish.truncated);
        assert_eq!(env.action_mask(0)?, vec![false; 9]);
        assert_eq!(env.action_mask(1)?, vec![false; 9]);
        let ended = env.get_state()?;
        assert!(env.step_index(1, 8).is_err());
        assert_eq!(env.get_state()?, ended);
    }
    env.reset(7, None)?;
    assert_eq!(env.get_state()?, Env::<TicTacToe>::new(7)?.get_state()?);
    Ok(())
}

#[test]
fn capped_chess_checkpoint_restores_truncation_and_rejects_wrong_config() -> Result<(), GameError> {
    let config = ChessConfig {
        max_plies: 1,
        ..ChessConfig::default()
    };
    let mut env = Env::<ChessGame>::with_config(config, 0)?;
    let step = env.step_string(0, "e2e4")?;
    assert!(step.truncated);
    assert!(!step.terminated);
    assert_eq!(step.rewards, vec![0.0, 0.0]);
    let saved = env.get_state()?;
    env.reset(0, None)?;
    assert!(!env.truncated());
    env.set_state(&saved)?;
    assert!(env.truncated());
    assert!(!env.terminated());
    assert!(env.current_players().is_empty());
    let mut different = Env::<ChessGame>::new(0)?;
    assert!(different.set_state(&saved).is_err());
    assert_eq!(
        different.get_state()?,
        Env::<ChessGame>::new(0)?.get_state()?
    );
    Ok(())
}

#[test]
fn checkpoint_metadata_and_state_are_validated_atomically() -> Result<(), GameError> {
    let mut env = Env::<TicTacToe>::new(0)?;
    let saved = env.get_state()?;
    let invalid = [
        EnvSnapshot {
            format_version: 255,
            ..saved.clone()
        },
        EnvSnapshot {
            game_id: "chess".into(),
            ..saved.clone()
        },
        EnvSnapshot {
            engine_version: "unknown".into(),
            ..saved.clone()
        },
        EnvSnapshot {
            state: serde_json::json!({"invalid":true}),
            ..saved.clone()
        },
    ];
    for snapshot in invalid {
        assert!(env.set_state(&snapshot).is_err());
        assert_eq!(env.get_state()?, saved);
    }
    assert!(env.reset(0, Some("unparseable")).is_err());
    assert_eq!(env.get_state()?, saved);
    Ok(())
}

#[test]
fn seeded_sudoku_checkpoints_and_position_resets_preserve_the_puzzle() -> Result<(), GameError> {
    let config = SudokuConfig {
        size: 4,
        max_moves: Some(2),
        ..SudokuConfig::default()
    };
    let mut env = Env::<Sudoku>::with_config(config.clone(), 17)?;
    let saved = env.get_state()?;
    let encoded = serde_json::to_string(&saved)?;
    let checkpoint: EnvSnapshot = serde_json::from_str(&encoded)?;
    let position = Sudoku::state_to_notation(env.state())?;
    env.reset(999, Some(&position))?;
    assert_eq!(env.get_state()?, saved);
    let action = env
        .legal_actions(0)?
        .into_iter()
        .find(|action| matches!(action, gfa_game_sudoku::Action::Place { .. }))
        .ok_or_else(|| GameError::illegal("missing placement"))?;
    env.step(0, &action)?;
    let continued = env.get_state()?;
    env.set_state(&checkpoint)?;
    env.step(0, &action)?;
    assert_eq!(env.get_state()?, continued);
    assert_eq!(
        env.observe(Viewer::Player(0))?,
        Sudoku::observe(env.state(), Viewer::Player(0))
    );
    env.reset(17, None)?;
    assert_eq!(
        env.get_state()?,
        Env::<Sudoku>::with_config(config, 17)?.get_state()?
    );
    Ok(())
}

#[test]
fn registry_training_preserves_native_semantics() -> Result<(), GameError> {
    let registry = crate::registry()?;
    assert!(registry
        .training_env("missing", &serde_json::json!({}), 0)
        .is_err());
    assert!(registry
        .training_env("tictactoe", &serde_json::json!({"bad":1}), 0)
        .is_err());
    for spec in registry.specs() {
        let config = if spec.id == "sudoku" {
            serde_json::json!({"size":4})
        } else {
            serde_json::json!({})
        };
        let mut env = registry.training_env(&spec.id, &config, 1)?;
        let game = registry.get(&spec.id)?;
        let before = env.get_state()?;
        let mut clone = env.clone();
        let seat = env.current_players()[0];
        let index = env
            .action_mask(seat)?
            .iter()
            .position(|legal| *legal)
            .ok_or_else(|| GameError::illegal("missing action"))? as u32;
        let step = env.step_index(seat, index)?;
        let (expected, events) =
            game.apply(&before.state, seat, &serde_json::json!({"index":index}))?;
        assert_eq!(env.get_state()?.state, expected);
        assert_eq!(step.events, events);
        assert_eq!(step.terminated, game.is_terminal(&expected)?);
        assert_eq!(step.truncated, game.is_truncated(&expected)?);
        assert_eq!(clone.get_state()?, before);
        clone.set_state(&env.get_state()?)?;
        assert_eq!(
            clone.observe(Viewer::Player(seat))?,
            env.observe(Viewer::Player(seat))?
        );
    }
    Ok(())
}


#[test]
fn numeric_projection_keeps_viewer_validation_and_plugin_fallbacks() -> Result<(), GameError> {
    let registry = crate::registry()?;
    for spec in registry.specs() {
        let mut env = registry.training_env(&spec.id, &serde_json::json!({}), 42)?;
        for _ in 0..4 {
            for viewer in [Viewer::Player(0), Viewer::Spectator] {
                assert_eq!(env.observe_tensor(viewer)?, env.observe(viewer)?.tensor);
            }
            assert!(env.observe_tensor(Viewer::Player(255)).is_err());
            let Some(seat) = env.current_players().first().copied() else { break; };
            let index = env.action_mask(seat)?.iter().position(|legal| *legal)
                .ok_or_else(|| GameError::illegal("missing action"))? as u32;
            env.step_index(seat, index)?;
        }
    }
    Ok(())
}

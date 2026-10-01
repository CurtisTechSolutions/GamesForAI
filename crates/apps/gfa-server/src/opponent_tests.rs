use gfa_core::{DynGame, GameError, SeededRng, Viewer};
use gfa_opponents::{Algorithm, Clock, Opponent, PlayerTurn, Random, SearchLimits, SearchOpponent};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixed;
impl Clock for Fixed {
    fn now_ms(&self) -> u64 {
        0
    }
}
struct Advancing(AtomicU64);
impl Clock for Advancing {
    fn now_ms(&self) -> u64 {
        self.0.fetch_add(100, Ordering::Relaxed)
    }
}
fn game(id: &str) -> Result<Arc<dyn DynGame>, GameError> {
    gfa_games::registry()?.get(id)
}
fn limits() -> SearchLimits {
    SearchLimits {
        nodes: 200_000,
        depth: 9,
        time_ms: 5_000,
        seed: 42,
    }
}
fn choose(
    game: &dyn DynGame,
    player: &dyn Opponent,
    state: &Value,
    limits: SearchLimits,
    clock: &dyn Clock,
) -> Result<gfa_opponents::ActionChoice, GameError> {
    let seat = game.current_players(state)?[0];
    player.choose_action(
        &PlayerTurn {
            seat,
            observation: &game.observe(state, Viewer::Player(seat))?,
            legal_actions: &game.legal_actions(state, seat)?,
        },
        limits,
        clock,
    )
}

#[test]
fn observation_reconstruction_round_trips_all_reachable_playout_states() -> TestResult {
    for id in ["tictactoe", "connect4"] {
        let game = game(id)?;
        let mut rng = SeededRng::new(42);
        for _ in 0..16 {
            let mut state = game.initial_state(&json!({}), 0)?;
            loop {
                for viewer in [Viewer::Player(0), Viewer::Player(1), Viewer::Spectator] {
                    let observation = game.observe(&state, viewer)?;
                    let restored =
                        game.state_from_observation(&json!({}), &observation, viewer, 71)?;
                    assert_eq!(restored, state);
                }
                if game.is_terminal(&state)? {
                    break;
                }
                let seat = game.current_players(&state)?[0];
                let actions = game.legal_actions(&state, seat)?;
                let index = rng.index(actions.len()).ok_or("empty")?;
                state = game.apply(&state, seat, &actions[index].json)?.0;
            }
        }
    }
    Ok(())
}

#[test]
fn minimax_never_loses_against_random_from_either_seat() -> TestResult {
    let game = game("tictactoe")?;
    let player = SearchOpponent::new(game.clone(), json!({}), Algorithm::Minimax)?;
    for our_seat in 0..2 {
        for seed in 0..6 {
            let mut state = game.initial_state(&json!({}), seed)?;
            while !game.is_terminal(&state)? {
                let seat = game.current_players(&state)?[0];
                let opponent: &dyn Opponent = if seat == our_seat { &player } else { &Random };
                let action = choose(
                    game.as_ref(),
                    opponent,
                    &state,
                    SearchLimits {
                        seed: seed * 13 + game.legal_actions(&state, seat)?.len() as u64,
                        ..limits()
                    },
                    &Fixed,
                )?;
                state = game.apply(&state, seat, &action.action.json)?.0;
            }
            assert!(game.returns(&state)?[usize::from(our_seat)] >= 0.0);
        }
    }
    Ok(())
}

#[test]
fn both_searches_find_forced_wins_and_respect_limits() -> TestResult {
    for id in ["tictactoe", "connect4"] {
        let game = game(id)?;
        let mut state = game.initial_state(&json!({}), 0)?;
        let (line, win): (Vec<Value>, _) = if id == "tictactoe" {
            (
                vec![json!("r1c1"), json!("r2c1"), json!("r1c2"), json!("r2c2")],
                "r1c3",
            )
        } else {
            (
                vec![
                    json!("1"),
                    json!("2"),
                    json!("1"),
                    json!("2"),
                    json!("1"),
                    json!("3"),
                ],
                "1",
            )
        };
        for action in line {
            let seat = game.current_players(&state)?[0];
            state = game.apply(&state, seat, &action)?.0;
        }
        for algorithm in [Algorithm::Minimax, Algorithm::Mcts] {
            let player = SearchOpponent::new(game.clone(), json!({}), algorithm)?;
            let budget = SearchLimits {
                nodes: 15_000,
                depth: 8,
                ..limits()
            };
            let first = choose(game.as_ref(), &player, &state, budget, &Fixed)?;
            assert_eq!(first.action.string, win, "{id} {algorithm:?}");
            assert!(first.info.nodes <= budget.nodes);
            assert_eq!(
                first,
                choose(game.as_ref(), &player, &state, budget, &Fixed)?
            );
            let tiny = choose(
                game.as_ref(),
                &player,
                &state,
                SearchLimits { nodes: 1, ..budget },
                &Fixed,
            )?;
            assert!(tiny.info.budget_exhausted);
            assert!(tiny.info.nodes <= 1);
            let timed = choose(
                game.as_ref(),
                &player,
                &state,
                SearchLimits {
                    time_ms: 1,
                    ..budget
                },
                &Advancing(AtomicU64::new(0)),
            )?;
            assert!(timed.info.budget_exhausted);
            assert_eq!(timed.info.nodes, 0);
            assert!(game.legal_actions(&state, 0)?.contains(&timed.action));
        }
    }
    Ok(())
}

#[test]
fn players_reject_invalid_limits_and_inconsistent_turns() -> TestResult {
    let game = game("tictactoe")?;
    let state = game.initial_state(&json!({}), 0)?;
    let player = SearchOpponent::new(game.clone(), json!({}), Algorithm::Minimax)?;
    assert!(choose(
        game.as_ref(),
        &Random,
        &state,
        SearchLimits {
            nodes: 0,
            ..limits()
        },
        &Fixed
    )
    .is_err());
    let mut actions = game.legal_actions(&state, 0)?;
    actions.pop();
    assert!(player
        .choose_action(
            &PlayerTurn {
                seat: 0,
                observation: &game.observe(&state, Viewer::Player(0))?,
                legal_actions: &actions,
            },
            limits(),
            &Fixed
        )
        .is_err());
    assert!(SearchLimits::for_level(0, 0).is_err());
    assert!(SearchLimits::for_level(11, 0).is_err());
    for level in 2..=10 {
        assert!(
            SearchLimits::for_level(level, 0)?.nodes > SearchLimits::for_level(level - 1, 0)?.nodes
        );
    }
    let first = choose(game.as_ref(), &Random, &state, limits(), &Fixed)?;
    assert_eq!(
        first,
        choose(game.as_ref(), &Random, &state, limits(), &Fixed)?
    );
    Ok(())
}


#[test]
fn search_treats_length_caps_as_leaves_without_changing_game_endings() -> TestResult {
    let game = game("chess")?;
    for max_plies in [1, 2] {
        let config = json!({"max_plies": max_plies});
        for algorithm in [Algorithm::Minimax, Algorithm::Mcts] {
            let player = SearchOpponent::new(game.clone(), config.clone(), algorithm)?;
            let mut state = game.initial_state(&config, 0)?;
            for _ in 0..max_plies {
                let budget = SearchLimits { nodes: 128, depth: 4, ..limits() };
                let first = choose(game.as_ref(), &player, &state, budget, &Fixed)?;
                assert_eq!(first, choose(game.as_ref(), &player, &state, budget, &Fixed)?);
                assert!(first.info.nodes <= budget.nodes);
                let seat = game.current_players(&state)?[0];
                state = game.apply(&state, seat, &first.action.json)?.0;
            }
            assert!(game.is_truncated(&state)?);
            assert!(!game.is_terminal(&state)?);
            assert_eq!(game.returns(&state)?, vec![0.0, 0.0]);
        }
    }
    Ok(())
}

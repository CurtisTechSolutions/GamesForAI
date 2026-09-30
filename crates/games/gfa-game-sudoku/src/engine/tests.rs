use super::*;
use gfa_core::serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const PUZZLE: &str =
    "530070000600195000098000060800060003400803001700020006060000280000419005000080079";
const SOLUTION: &str =
    "534678912672195348198342567859761423426853791713924856961537284287419635345286179";

fn config() -> Config {
    Config {
        puzzle: Some(PUZZLE.into()),
        ..Config::default()
    }
}

fn play(state: &mut State, text: &str) -> Result<StepEvents, GameError> {
    let action = Sudoku::action_from_string(state, text)?;
    Sudoku::apply(state, 0, &action)
}

#[test]
fn variants_pass_full_contract_with_bounded_episodes() -> TestResult {
    for size in [4, 6, 9] {
        gfa_testkit::conformance_with_config::<Sudoku>(&Config {
            size,
            max_moves: Some(4),
            ..Config::default()
        })?;
    }
    Ok(())
}

#[test]
fn solves_unique_puzzle_and_reports_single_player_return() -> TestResult {
    let mut state = Sudoku::new_initial_state(&config(), 7)?;
    for (i, byte) in SOLUTION.bytes().enumerate() {
        if state.puzzle[i] == 0 {
            play(
                &mut state,
                &format!("r{}c{}={}", i / 9 + 1, i % 9 + 1, byte - b'0'),
            )?;
        }
    }
    assert!(Sudoku::is_terminal(&state));
    assert!(Sudoku::current_players(&state).is_empty());
    assert_eq!(Sudoku::returns(&state), vec![1.0]);
    assert_eq!(
        Sudoku::observe(&state, Viewer::Player(0)).json["outcome"],
        "solved"
    );
    assert_eq!(state.moves, 51);
    assert!(play(&mut state, "r1c3=0").is_err());
    let imported = Sudoku::state_from_notation(&Config::default(), SOLUTION)?;
    assert!(Sudoku::is_terminal(&imported));
    Ok(())
}

#[test]
fn notes_are_free_edits_round_trip_and_rejections_are_atomic() -> TestResult {
    let cfg = config();
    let mut state = Sudoku::new_initial_state(&cfg, 0)?;
    play(&mut state, "r1c3+4")?;
    assert_eq!(state.moves, 0);
    assert_eq!(state.notes[2], 8);
    play(&mut state, "r1c3-4")?;
    assert_eq!(state.moves, 0);
    play(&mut state, "r1c3=5")?;
    assert_eq!(
        Sudoku::observe(&state, Viewer::Player(0)).json["mistakes"],
        json!(null)
    );
    assert!(Sudoku::observe(&state, Viewer::Player(0)).json["conflicts"]
        .as_array()
        .is_some_and(|v| v.contains(&json!(2))));
    let serialized = serde_json::to_value(&state)?;
    let notation = Sudoku::state_to_notation(&state)?;
    assert!(notation.starts_with('{'));
    assert_eq!(
        serde_json::to_value(Sudoku::state_from_notation(&cfg, &notation)?)?,
        serialized
    );
    let reconstructed = Sudoku::state_from_observation(
        &cfg,
        &Sudoku::observe(&state, Viewer::Player(0)),
        Viewer::Player(0),
        99,
    )?;
    assert_eq!(serde_json::to_value(&reconstructed)?, serialized);
    for text in ["r1c1=4", "r1c3+3", "r1c3=5"] {
        assert!(play(&mut state, text).is_err());
        assert_eq!(serde_json::to_value(&state)?, serialized);
    }
    let action = Action::Erase { row: 1, col: 3 };
    assert!(Sudoku::apply(&mut state, 1, &action).is_err());
    assert_eq!(serde_json::to_value(&state)?, serialized);
    play(&mut state, "r1c3=0")?;
    assert_eq!(state.erasures, 1);
    assert_eq!(state.wrong, 1);
    assert_eq!(
        Sudoku::observe(&state, Viewer::Player(0)),
        Sudoku::observe(&state, Viewer::Spectator)
    );
    let encoded = serde_json::to_string(&Sudoku::observe(&state, Viewer::Player(0)).json)?;
    assert!(!encoded.contains("solution"));
    Ok(())
}

#[test]
fn rule_check_names_conflict_and_solution_check_limits_mistakes() -> TestResult {
    let mut state = Sudoku::new_initial_state(
        &Config {
            mistake_policy: "rule_check".into(),
            ..config()
        },
        0,
    )?;
    let before = serde_json::to_value(&state)?;
    let error = play(&mut state, "r1c3=5")
        .err()
        .ok_or("expected conflict")?;
    assert_eq!(error.code, ErrorCode::IllegalAction);
    assert!(error.message.contains("r1c1"));
    assert_eq!(serde_json::to_value(&state)?, before);
    assert!(!Sudoku::legal_actions(&state, 0).contains(&Action::Place {
        row: 1,
        col: 3,
        digit: 5
    }));
    let mut state = Sudoku::new_initial_state(
        &Config {
            mistake_policy: "solution_check:2".into(),
            ..config()
        },
        0,
    )?;
    play(&mut state, "r1c3=5")?;
    assert!(!Sudoku::is_terminal(&state));
    play(&mut state, "r1c3=6")?;
    assert!(Sudoku::is_terminal(&state));
    assert_eq!(Sudoku::returns(&state), vec![0.0]);
    assert_eq!(
        Sudoku::observe(&state, Viewer::Player(0)).json["outcome"],
        "mistake_limit"
    );
    assert_eq!(
        Sudoku::observe(&state, Viewer::Player(0)).json["mistakes"],
        2
    );
    Ok(())
}

#[test]
fn move_cap_truncates_and_notes_do_not_consume_it() -> TestResult {
    let mut state = Sudoku::new_initial_state(
        &Config {
            max_moves: Some(1),
            ..config()
        },
        0,
    )?;
    for _ in 0..4 {
        assert!(!play(&mut state, "r1c3+4")?.truncated);
        assert!(!play(&mut state, "r1c3-4")?.truncated);
    }
    assert!(play(&mut state, "r1c3=4")?.truncated);
    assert!(!Sudoku::is_terminal(&state));
    assert!(Sudoku::current_players(&state).is_empty());
    assert!(Sudoku::legal_actions(&state, 0).is_empty());
    assert_eq!(state.moves, 1);
    assert_eq!(Sudoku::returns(&state), vec![0.0]);
    Ok(())
}

#[test]
fn untrusted_positions_configs_and_action_encodings_are_validated() -> TestResult {
    for cfg in [
        Config {
            size: 5,
            ..Config::default()
        },
        Config {
            mistake_policy: "solution_check:0".into(),
            ..config()
        },
        Config {
            max_moves: Some(0),
            ..config()
        },
        Config {
            puzzle: Some(".".repeat(81)),
            ..Config::default()
        },
        Config {
            puzzle: Some("11".to_owned() + &".".repeat(79)),
            ..Config::default()
        },
    ] {
        assert!(Sudoku::new_initial_state(&cfg, 0).is_err());
    }
    let state = Sudoku::new_initial_state(&config(), 0)?;
    assert!(serde_json::to_value(&state)?.get("solution").is_none());
    assert!(serde_json::to_value(&state)?
        .get("validated_puzzle")
        .is_none());
    for text in ["r0c1=2", "r1c1+0", "r1c1=10", "r1c1=2 ", "réc1=2", "r1c1*2"] {
        assert!(Sudoku::action_from_string(&state, text).is_err());
    }
    for index in 0..2268 {
        let action = Sudoku::action_from_index(&state, index)?;
        assert_eq!(Sudoku::action_to_index(&action), index);
        assert_eq!(
            Sudoku::action_from_string(&state, &Sudoku::action_to_string(&state, &action))?,
            action
        );
    }
    assert!(Sudoku::action_from_index(&state, 2268).is_err());
    assert_eq!(
        Sudoku::action_to_index(&Action::Place {
            row: 0,
            col: 1,
            digit: 1
        }),
        u32::MAX
    );
    for (key, value) in [
        ("wrong", json!(1)),
        ("notes", json!([1])),
        ("attempts", json!({"99999":1})),
        (
            "grade",
            json!({"difficulty":"expert","techniques":[],"logical_steps":0}),
        ),
    ] {
        let mut encoded = serde_json::to_value(&state)?;
        encoded[key] = value;
        let forged: State = serde_json::from_value(encoded)?;
        assert!(Sudoku::validate_state(&forged).is_err(), "{key}");
    }
    Ok(())
}

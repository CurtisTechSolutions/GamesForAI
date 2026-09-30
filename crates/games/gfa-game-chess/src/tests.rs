use super::*;
use std::collections::HashSet;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn play(state: &mut State, text: &str) -> Result<StepEvents, GameError> {
    let player = ChessGame::current_players(state)
        .first()
        .copied()
        .ok_or_else(|| GameError::illegal("No player"))?;
    let action = ChessGame::action_from_string(state, text)?;
    ChessGame::apply(state, player, &action)
}

fn from(fen: &str) -> Result<State, GameError> {
    ChessGame::state_from_notation(&Config::default(), fen)
}

fn has(state: &State, text: &str) -> bool {
    ChessGame::current_players(state).first().is_some_and(|&p| {
        ChessGame::legal_actions(state, p)
            .iter()
            .any(|a| ChessGame::action_to_string(state, a) == text)
    })
}

fn cycle(state: &mut State) -> Result<(), GameError> {
    for m in ["g1f3", "g8f6", "f3g1", "f6g8"] {
        play(state, m)?;
    }
    Ok(())
}

#[test]
fn conformance_and_observation_reconstruction() -> TestResult {
    let config = Config {
        max_plies: 8,
        ..Config::default()
    };
    gfa_testkit::conformance_with_config::<ChessGame>(&config)?;
    let mut state = ChessGame::new_initial_state(&config, 9)?;
    play(&mut state, "e2e4")?;
    let observation = ChessGame::observe(&state, Viewer::Player(1));
    let rebuilt = ChessGame::state_from_observation(&config, &observation, Viewer::Player(1), 987)?;
    assert_eq!(
        serde_json::to_value(rebuilt)?,
        serde_json::to_value(&state)?
    );
    let mut tampered = observation;
    tampered.json["fen"] = json!("forged");
    assert!(ChessGame::state_from_observation(&config, &tampered, Viewer::Player(1), 0).is_err());
    Ok(())
}

fn perft(state: &State, depth: u8) -> Result<u64, GameError> {
    if depth == 0 {
        return Ok(1);
    }
    let player = ChessGame::current_players(state)[0];
    let actions = ChessGame::legal_actions(state, player);
    if depth == 1 {
        return Ok(actions.len() as u64);
    }
    let mut total = 0;
    for action in actions {
        let mut next = state.clone();
        ChessGame::apply(&mut next, player, &action)?;
        total += perft(&next, depth - 1)?;
    }
    Ok(total)
}

#[test]
fn standard_move_counts_and_kiwipete() -> TestResult {
    let state = ChessGame::new_initial_state(&Config::default(), 0)?;
    for (depth, expected) in [(1, 20), (2, 400), (3, 8902), (4, 197281)] {
        assert_eq!(perft(&state, depth)?, expected);
    }
    let kiwipete = from("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")?;
    for (depth, expected) in [(1, 48), (2, 2039), (3, 97862)] {
        assert_eq!(perft(&kiwipete, depth)?, expected);
    }
    Ok(())
}

#[test]
fn castling_moves_rook_and_rejects_aliases_or_attacked_path() -> TestResult {
    let mut state = from("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1")?;
    assert!(has(&state, "e1g1") && has(&state, "e1c1"));
    let before = serde_json::to_value(&state)?;
    assert!(play(&mut state, "e1h1").is_err());
    assert_eq!(before, serde_json::to_value(&state)?);
    play(&mut state, "e1g1")?;
    let view = ChessGame::observe(&state, Viewer::Spectator);
    assert_eq!(view.json["pieces"]["g1"], "K");
    assert_eq!(view.json["pieces"]["f1"], "R");
    assert_eq!(view.json["last_move_san"], "O-O");
    assert_eq!(
        view.json["fen"]
            .as_str()
            .and_then(|s| s.split_whitespace().nth(2)),
        Some("kq")
    );
    let attacked = from("r3kr1r/8/8/8/8/8/8/R3K2R w KQkq - 0 1")?;
    assert!(!has(&attacked, "e1g1"));
    assert!(has(&attacked, "e1c1"));
    Ok(())
}

#[test]
fn en_passant_expires_and_cannot_expose_the_king() -> TestResult {
    let mut state = ChessGame::new_initial_state(&Config::default(), 0)?;
    for m in ["e2e4", "a7a6", "e4e5", "d7d5"] {
        play(&mut state, m)?;
    }
    assert!(has(&state, "e5d6"));
    let mut expired = state.clone();
    play(&mut expired, "g1f3")?;
    play(&mut expired, "a6a5")?;
    assert!(!has(&expired, "e5d6"));
    play(&mut state, "e5d6")?;
    let view = ChessGame::observe(&state, Viewer::Spectator);
    assert_eq!(view.json["pieces"]["d6"], "P");
    assert!(view.json["pieces"].get("d5").is_none());
    let pinned = from("k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1")?;
    assert!(!has(&pinned, "e5d6"));
    let mut no_ep = pinned
        .derived()?
        .position
        .clone()
        .to_setup(EnPassantMode::Always);
    no_ep.ep_square = None;
    let no_ep = Fen::try_from_setup(no_ep)
        .map_err(|e| format!("{e:?}"))?
        .into_position::<Chess>(CastlingMode::Standard)?;
    assert_eq!(key(&pinned.derived()?.position), key(&no_ep));
    Ok(())
}

#[test]
fn promotions_have_unique_indices_for_both_colors() -> TestResult {
    for (position, prefix) in [
        ("4k3/P7/8/8/8/8/7p/4K3 w - - 0 1", "a7a8"),
        ("4k3/P7/8/8/8/8/7p/4K3 b - - 0 1", "h2h1"),
    ] {
        let state = from(position)?;
        let mut indices = HashSet::new();
        for suffix in ['q', 'r', 'b', 'n'] {
            let text = format!("{prefix}{suffix}");
            assert!(has(&state, &text));
            let action = ChessGame::action_from_string(&state, &text)?;
            let index = ChessGame::action_to_index(&action);
            assert!(indices.insert(index));
            assert_eq!(ChessGame::action_from_index(&state, index)?, action);
            let mut next = state.clone();
            play(&mut next, &text)?;
            ChessGame::validate_state(&next)?;
        }
        assert!(!has(&state, prefix));
    }
    Ok(())
}

#[test]
fn mate_stalemate_material_and_move_cap_are_distinct() -> TestResult {
    let mut state = ChessGame::new_initial_state(&Config::default(), 0)?;
    for m in ["f2f3", "e7e5", "g2g4", "d8h4"] {
        play(&mut state, m)?;
    }
    assert!(ChessGame::is_terminal(&state));
    assert_eq!(ChessGame::returns(&state), vec![-1.0, 1.0]);
    assert_eq!(state.derived()?.last_move_san.as_deref(), Some("Qh4#"));
    for position in [
        "7k/5K2/6Q1/8/8/8/8/8 b - - 0 1",
        "7k/8/8/8/8/8/8/K7 w - - 0 1",
        "7k/8/8/8/8/8/8/KB6 w - - 0 1",
    ] {
        let state = from(position)?;
        assert!(ChessGame::is_terminal(&state));
        assert!(ChessGame::current_players(&state).is_empty());
        assert_eq!(ChessGame::returns(&state), vec![0.0, 0.0]);
    }
    let config = Config {
        max_plies: 1,
        ..Config::default()
    };
    let mut capped = ChessGame::new_initial_state(&config, 0)?;
    assert!(play(&mut capped, "e2e4")?.truncated);
    assert!(!ChessGame::is_terminal(&capped));
    assert!(ChessGame::current_players(&capped).is_empty());
    assert_eq!(capped.derived()?.status, Status::MoveLimit);
    Ok(())
}

#[test]
fn repetition_claims_include_intended_moves_and_fivefold_is_automatic() -> TestResult {
    let mut state = ChessGame::new_initial_state(&Config::default(), 0)?;
    cycle(&mut state)?;
    for m in ["g1f3", "g8f6", "f3g1"] {
        play(&mut state, m)?;
    }
    assert!(has(&state, "claim_draw:f6g8"));
    let mut prospective = state.clone();
    let before = fen(&prospective.derived()?.position);
    play(&mut prospective, "claim_draw:f6g8")?;
    assert_eq!(fen(&prospective.derived()?.position), before);
    assert!(ChessGame::is_terminal(&prospective));
    play(&mut state, "f6g8")?;
    assert!(has(&state, "claim_draw"));
    let mut claimed = state.clone();
    play(&mut claimed, "claim_draw")?;
    assert_eq!(claimed.derived()?.status, Status::ClaimedDraw);
    cycle(&mut state)?;
    assert!(!ChessGame::is_terminal(&state));
    cycle(&mut state)?;
    assert_eq!(state.derived()?.status, Status::FivefoldRepetition);
    assert!(ChessGame::is_terminal(&state));
    Ok(())
}

#[test]
fn fifty_and_seventy_five_move_rules_and_mate_precedence() -> TestResult {
    let state = from("7k/8/8/8/8/8/R7/K7 w - - 99 1")?;
    assert!(!has(&state, "claim_draw"));
    assert!(has(&state, "claim_draw:a2b2"));
    let mut current = state.clone();
    play(&mut current, "a2b2")?;
    assert!(has(&current, "claim_draw"));
    play(&mut current, "claim_draw")?;
    assert!(ChessGame::is_terminal(&current));
    let mut automatic = from("7k/8/8/8/8/8/R7/K7 w - - 149 1")?;
    play(&mut automatic, "a2b2")?;
    assert_eq!(automatic.derived()?.status, Status::SeventyFiveMove);
    let mut mate = from("7k/8/5KQ1/8/8/8/8/8 w - - 149 1")?;
    play(&mut mate, "g6g7")?;
    assert_eq!(mate.derived()?.status, Status::Checkmate { winner: 0 });
    Ok(())
}

#[test]
fn lossless_history_import_rejects_forgery_and_post_terminal_play() -> TestResult {
    let mut state = ChessGame::new_initial_state(&Config::default(), 0)?;
    cycle(&mut state)?;
    cycle(&mut state)?;
    let exported = ChessGame::state_to_notation(&state)?;
    assert!(exported.starts_with('{'));
    let restored = ChessGame::state_from_notation(&Config::default(), &exported)?;
    assert_eq!(
        ChessGame::observe(&restored, Viewer::Spectator),
        ChessGame::observe(&state, Viewer::Spectator)
    );
    let public = ChessGame::public_position(&state)?.ok_or("FEN")?;
    assert!(!has(&from(&public)?, "claim_draw"));
    let mut fake = serde_json::to_value(&state)?;
    fake["actions"][0]["uci"] = json!("e2e5");
    let forged: State = serde_json::from_value(fake)?;
    assert!(ChessGame::validate_state(&forged).is_err());
    play(&mut state, "claim_draw")?;
    let mut fake = serde_json::to_value(&state)?;
    fake["actions"]
        .as_array_mut()
        .ok_or("actions")?
        .push(json!({"type":"move","uci":"e2e4"}));
    let forged: State = serde_json::from_value(fake)?;
    assert!(ChessGame::validate_state(&forged).is_err());
    Ok(())
}

#[test]
fn invalid_positions_encodings_and_seats_are_atomic() -> TestResult {
    for position in [
        "8/8/8/8/8/8/8/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w K - 0 1",
        "4k3/8/8/8/8/8/8/P3K3 w - - 0 1",
        "8/8/8/8/8/8/4k3/4K3 w - - 0 1",
        "not a FEN",
    ] {
        assert!(from(position).is_err(), "{position}");
    }
    let mut state = ChessGame::new_initial_state(&Config::default(), 0)?;
    let original = serde_json::to_value(&state)?;
    for text in [
        "0000",
        "e4",
        "E2E4",
        "e2e5",
        "e2e4q",
        "claim_draw",
        "claim_draw:e2e4",
    ] {
        assert!(play(&mut state, text).is_err(), "{text}");
        assert_eq!(serde_json::to_value(&state)?, original);
    }
    let action = ChessGame::action_from_string(&state, "e2e4")?;
    assert_eq!(
        ChessGame::apply(&mut state, 1, &action)
            .err()
            .ok_or("error")?
            .code,
        ErrorCode::NotYourTurn
    );
    assert_eq!(serde_json::to_value(&state)?, original);
    assert!(ChessGame::legal_actions(&state, 9).is_empty());
    Ok(())
}


#[test]
fn uci_export_preserves_initial_position_and_repetition_history() -> TestResult {
    let mut state = ChessGame::new_initial_state(&Config::default(), 0)?;
    cycle(&mut state)?;
    cycle(&mut state)?;
    let position = ChessGame::uci_position(&state)?.ok_or("UCI position")?;
    assert_eq!(position.initial_fen, state.initial_fen);
    assert_eq!(position.moves, ["g1f3","g8f6","f3g1","f6g8","g1f3","g8f6","f3g1","f6g8"]);
    assert!(!position.chess960);
    let view = ChessGame::observe(&state, Viewer::Player(0));
    let rebuilt = ChessGame::state_from_observation(&Config::default(), &view, Viewer::Player(0), 5)?;
    assert_eq!(ChessGame::uci_position(&rebuilt)?, Some(position));
    Ok(())
}

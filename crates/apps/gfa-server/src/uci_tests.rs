use gfa_core::{
    serde_json::json, ActionChoice, DynGame, OpponentError, PlayerTurn, SearchLimits, Viewer,
};
use gfa_engine_uci::{validate_recommendation, Bound, Info, Score, SearchResult};
use std::sync::Arc;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn game() -> Result<Arc<dyn DynGame>, gfa_core::GameError> {
    gfa_games::registry()?.get("chess")
}
fn result() -> SearchResult {
    SearchResult {
        engine_name: "Fixture".into(),
        best_move: "e2e4".into(),
        ponder: Some("e7e5".into()),
        variations: vec![Info {
            depth: Some(3),
            nodes: Some(100),
            time_ms: Some(10),
            multipv: Some(1),
            score: Some(Score::Centipawns(23)),
            bound: Bound::Exact,
            wdl: Some([300, 500, 200]),
            pv: vec!["e2e4".into(), "e7e5".into(), "g1f3".into()],
        }],
        nodes: 100,
        elapsed_ms: 10,
    }
}
fn validate(
    game: &dyn DynGame,
    state: &gfa_core::serde_json::Value,
    result: &SearchResult,
) -> Result<ActionChoice, OpponentError> {
    let seat = game.current_players(state)?[0];
    validate_recommendation(
        game,
        &json!({}),
        &PlayerTurn {
            seat,
            observation: &game.observe(state, Viewer::Player(seat))?,
            legal_actions: &game.legal_actions(state, seat)?,
        },
        SearchLimits {
            nodes: 1000,
            depth: 8,
            time_ms: 5000,
            seed: 7,
        },
        result,
    )
}
#[test]
fn validates_bestmove_and_continuations_without_converting_centipawns_to_returns() -> TestResult {
    let game = game()?;
    let state = game.initial_state(&json!({}), 7)?;
    let choice = validate(game.as_ref(), &state, &result())?;
    assert_eq!(choice.action.string, "e2e4");
    assert_eq!(choice.info.evaluation, None);
    assert_eq!(choice.info.principal_variation, ["e2e4", "e7e5", "g1f3"]);
    let advice = choice.info.advice.ok_or("missing engine diagnostics")?;
    assert_eq!(
        advice.details["variations"][0]["score"],
        json!({"unit":"centipawns","value":23})
    );
    assert_eq!(advice.details["variations"][0]["perspective_seat"], 0);
    assert_eq!(state, game.initial_state(&json!({}), 7)?);
    Ok(())
}
#[test]
fn illegal_engine_moves_pv_and_ponder_are_provider_errors() -> TestResult {
    let game = game()?;
    let state = game.initial_state(&json!({}), 7)?;
    let mut bad = result();
    bad.best_move = "e2e5".into();
    assert_eq!(
        validate(game.as_ref(), &state, &bad),
        Err(OpponentError::InvalidResponse)
    );
    bad = result();
    bad.variations[0].pv[1] = "e2e3".into();
    assert_eq!(
        validate(game.as_ref(), &state, &bad),
        Err(OpponentError::InvalidResponse)
    );
    bad = result();
    bad.ponder = Some("e2e3".into());
    assert_eq!(
        validate(game.as_ref(), &state, &bad),
        Err(OpponentError::InvalidResponse)
    );
    Ok(())
}
#[test]
fn black_scores_keep_the_root_seat_and_mate_stops_the_pv() -> TestResult {
    let game = game()?;
    let mut state = game.initial_state(&json!({}), 7)?;
    for token in ["f2f3", "e7e5", "g2g4"] {
        let seat = game.current_players(&state)?[0];
        state = game.apply(&state, seat, &json!(token))?.0;
    }
    let mut output = result();
    output.best_move = "d8h4".into();
    output.ponder = None;
    output.variations[0].pv = vec!["d8h4".into()];
    output.variations[0].score = Some(Score::Mate(1));
    let choice = validate(game.as_ref(), &state, &output)?;
    let advice = choice.info.advice.ok_or("missing diagnostics")?;
    assert_eq!(advice.details["variations"][0]["perspective_seat"], 1);
    assert_eq!(
        advice.details["variations"][0]["score"],
        json!({"unit":"mate_moves","value":1})
    );
    output.variations[0].pv.push("a2a3".into());
    assert_eq!(
        validate(game.as_ref(), &state, &output),
        Err(OpponentError::InvalidResponse)
    );
    Ok(())
}
#[test]
fn multipv_retains_only_legal_lines_and_selected_move_diagnostics() -> TestResult {
    let game = game()?;
    let state = game.initial_state(&json!({}), 7)?;
    let mut output = result();
    let mut second = output.variations[0].clone();
    second.multipv = Some(2);
    second.pv = vec!["d2d4".into(), "d7d5".into()];
    second.score = Some(Score::Centipawns(10));
    output.variations.push(second);
    output.best_move = "d2d4".into();
    output.ponder = Some("d7d5".into());
    let choice = validate(game.as_ref(), &state, &output)?;
    assert_eq!(choice.info.principal_variation, ["d2d4", "d7d5"]);
    assert_eq!(
        choice.info.advice.ok_or("missing diagnostics")?.details["variations"]
            .as_array()
            .ok_or("missing variations")?
            .len(),
        2
    );
    Ok(())
}


#[test]
fn analysis_ranks_all_variations_and_rejects_duplicate_roots() -> TestResult {
    let game = game()?;
    let state = game.initial_state(&json!({}), 7)?;
    let observation = game.observe(&state, Viewer::Player(0))?;
    let legal = game.legal_actions(&state, 0)?;
    let turn = PlayerTurn { seat: 0, observation: &observation, legal_actions: &legal };
    let limits = SearchLimits { nodes: 1000, depth: 8, time_ms: 5000, seed: 7 };
    let mut output = result();
    let mut second = output.variations[0].clone();
    second.multipv = Some(2);
    second.pv = vec!["d2d4".into(), "d7d5".into()];
    output.variations.insert(0, second);
    let choices = gfa_engine_uci::validate_analysis(game.as_ref(), &json!({}), &turn, limits, &output)?;
    assert_eq!(choices.iter().map(|choice| choice.action.string.as_str()).collect::<Vec<_>>(), ["e2e4", "d2d4"]);
    for choice in choices {
        assert_eq!(choice.info.principal_variation.first(), Some(&choice.action.string));
        assert_eq!(choice.info.advice.ok_or("diagnostics")?.details["variations"].as_array().ok_or("line")?.len(), 1);
    }
    output.variations[0].pv = output.variations[1].pv.clone();
    assert_eq!(gfa_engine_uci::validate_analysis(game.as_ref(), &json!({}), &turn, limits, &output), Err(OpponentError::InvalidResponse));
    Ok(())
}

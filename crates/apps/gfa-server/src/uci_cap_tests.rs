use gfa_core::{serde_json::json, PlayerTurn, SearchLimits, Viewer};
use gfa_engine_uci::{parse_line, validate_analysis, validate_recommendation, EngineLine, SearchResult};

#[test]
fn legal_uci_lines_end_at_the_training_cap() -> Result<(), Box<dyn std::error::Error>> {
    let game = gfa_games::registry()?.get("chess")?;
    let config = json!({"max_plies":1});
    let state = game.initial_state(&config, 42)?;
    let observation = game.observe(&state, Viewer::Player(0))?;
    let legal = game.legal_actions(&state, 0)?;
    let turn = PlayerTurn { seat: 0, observation: &observation, legal_actions: &legal };
    let EngineLine::Info(info) = parse_line("info depth 3 multipv 1 score cp 12 pv e2e4 e7e5 g1f3")? else {
        return Err("missing info".into());
    };
    let result = SearchResult {
        engine_name: "Fixture".into(),
        best_move: "e2e4".into(),
        ponder: Some("e7e5".into()),
        variations: vec![info],
        nodes: 40,
        elapsed_ms: 2,
    };
    let selected = validate_recommendation(game.as_ref(), &config, &turn, SearchLimits::for_level(3, 42)?, &result)?;
    assert_eq!(selected.action.string, "e2e4");
    assert_eq!(selected.info.principal_variation, ["e2e4"]);
    let advice = selected.info.advice.as_ref().ok_or("missing advice")?;
    assert_eq!(advice.details["variations"][0]["truncated"], true);
    let ranked = validate_analysis(game.as_ref(), &config, &turn, SearchLimits::for_level(3, 42)?, &result)?;
    assert_eq!(ranked[0].info.principal_variation, ["e2e4"]);
    // A bad move before the cap is still rejected, including an illegal PV root.
    let mut invalid = result.clone();
    invalid.best_move = "e2e5".into();
    assert!(validate_recommendation(game.as_ref(), &config, &turn, SearchLimits::for_level(3, 42)?, &invalid).is_err());
    let EngineLine::Info(info) = parse_line("info depth 1 pv e2e5")? else {
        return Err("missing info".into());
    };
    invalid = result;
    invalid.variations = vec![info];
    assert!(validate_analysis(game.as_ref(), &config, &turn, SearchLimits::for_level(3, 42)?, &invalid).is_err());
    Ok(())
}

use super::{mcp_play_tests::invoke, tests::{config,fixture}, *};
use gfa_core::Viewer;
use gfa_mcp::McpServer;
use serde_json::json;

#[tokio::test]
async fn mcp_planning_is_hypothetical_historical_and_accounted() -> Result<(),ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let adapter = McpServer::new(app.service.clone(),Viewer::Player(0))?;
    let created = invoke(&adapter,"create_match",json!({"game_id":"chess","seed":10,"allow_analysis":true}),false).await?;
    let id = created["state"]["match_id"].as_str().ok_or("id")?;
    let initial = app.service.get_state(id,Viewer::Player(0)).await?;
    let result = invoke(&adapter,"simulate_moves",json!({"match_id":id,"seed":11,"lines":[["e2e4","e7e5"],["e2e4","bad"],[]]}),false).await?;
    assert_eq!(result["lines"][0]["moves_applied"],2);
    assert_eq!(result["lines"][0]["state"]["turn"],2);
    assert_eq!(result["lines"][1]["moves_applied"],1);
    assert_eq!(result["lines"][1]["error"]["index"],1);
    assert_eq!(result["lines"][2]["moves_applied"],0);
    assert_eq!(app.service.get_state(id,Viewer::Player(0)).await?,initial);
    let usage = app.service.get_match(id).await?.assist_usage;
    assert_eq!(usage.len(),1);
    assert_eq!(usage[0].simulation_calls,1);
    assert_eq!(usage[0].simulated_moves,3);
    invoke(&adapter,"make_move",json!({"match_id":id,"action":"e2e4"}),false).await?;
    let current = app.service.get_state(id,Viewer::Player(0)).await?;
    let historical = invoke(&adapter,"simulate_moves",json!({"match_id":id,"from_turn":0,"lines":[["d2d4"]]}),false).await?;
    assert_eq!(historical["lines"][0]["moves_applied"],1);
    let analysis = invoke(&adapter,"analyze_position",json!({"match_id":id,"turn":0,"opponent":"minimax","nodes":100,"depth":1,"seed":9}),false).await?;
    assert!(!analysis["variations"].as_array().ok_or("variations")?.is_empty());
    let first = analysis["variations"][0]["action"].as_str().ok_or("action")?;
    assert!(initial.legal_actions.iter().any(|action|action.string == first));
    assert_eq!(app.service.get_state(id,Viewer::Player(0)).await?,current);
    assert_eq!(invoke(&adapter,"simulate_moves",json!({"match_id":id,"lines":vec![json!([]);17]}),true).await?["code"],"INVALID_CONFIG");
    invoke(&adapter,"analyze_position",json!({"match_id":id,"turn":999}),true).await?;
    let solo = invoke(&adapter,"create_match",json!({"game_id":"sudoku","seed":1,"allow_analysis":true}),false).await?;
    let solo_id = solo["state"]["match_id"].as_str().ok_or("id")?;
    let advice = invoke(&adapter,"analyze_position",json!({"match_id":solo_id}),false).await?;
    assert_eq!(advice["opponent"],"reference");
    assert!(advice["variations"][0]["advice"].is_object());
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn mcp_planning_obeys_assists_and_rejects_seat_overrides() -> Result<(),ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let player = McpServer::new(app.service.clone(),Viewer::Player(0))?;
    let spectator = McpServer::new(app.service.clone(),Viewer::Spectator)?;
    let created = invoke(&player,"create_match",json!({"game_id":"tictactoe","allow_analysis":false,"allow_simulation":false}),false).await?;
    let id = created["state"]["match_id"].as_str().ok_or("id")?;
    let before = app.service.get_state(id,Viewer::Player(0)).await?;
    for adapter in [&player,&spectator] {
        for (name,args) in [
            ("simulate_moves",json!({"match_id":id,"lines":[["r1c1"]]})),
            ("analyze_position",json!({"match_id":id})),
        ] { assert_eq!(invoke(adapter,name,args,true).await?["code"],"FORBIDDEN"); }
    }
    for (name,args) in [
        ("simulate_moves",json!({"match_id":id,"lines":[[]],"seat":1})),
        ("analyze_position",json!({"match_id":id,"seat":1})),
    ] { assert_eq!(invoke(&player,name,args,true).await?["code"],"INVALID_REQUEST"); }
    assert_eq!(app.service.get_state(id,Viewer::Player(0)).await?,before);
    assert!(app.service.get_match(id).await?.assist_usage.is_empty());
    app.store.close().await;
    Ok(())
}

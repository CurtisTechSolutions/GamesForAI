use super::{tests::{config, fixture}, *};
use gfa_core::Viewer;
use gfa_mcp::McpServer;
use serde_json::{json, Value};

async fn invoke(adapter: &McpServer, name: &str, args: Value, failed: bool) -> Result<Value, ServerError> {
    let tool = adapter.tool_definitions().into_iter().find(|tool|tool.name == name).ok_or("tool")?;
    let schema = serde_json::to_value(tool.output_schema.ok_or("schema")?)?;
    let validator = jsonschema::validator_for(&schema)?;
    let result = adapter.invoke(name,args).await?;
    assert_eq!(result.is_error,Some(failed),"{name}: {:?}",result.content);
    assert!(!result.content.is_empty());
    let structured = result.structured_content.ok_or("structured output")?;
    validator.validate(&structured).map_err(|error|error.to_string())?;
    Ok(structured[if failed { "error" } else { "data" }].clone())
}

#[tokio::test]
async fn mcp_plays_games_with_recovery_replies_idempotency_and_reasoning() -> Result<(),ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let adapter = McpServer::new(app.service.clone(),Viewer::Player(0))?;
    for game in ["tictactoe","connect4","chess"] {
        let created = invoke(&adapter,"create_match",json!({"game_id":game,"opponent":"random","seed":99}),false).await?;
        let id = created["state"]["match_id"].as_str().ok_or("match id")?;
        let initial = invoke(&adapter,"get_state",json!({"match_id":id}),false).await?;
        assert_eq!(initial["turn"],0);
        assert!(serde_json::to_vec(&initial)?.len() < 6000, "{game} state budget");
        assert!(initial.get("action_mask").is_none());
        assert!(initial.get("observation").is_none());
        let legal = invoke(&adapter,"get_legal_actions",json!({"match_id":id}),false).await?;
        let action = legal["legal_actions"][0].clone();
        assert!(action.is_string());
        let illegal = invoke(&adapter,"make_move",json!({"match_id":id,"action":"not-a-move"}),true).await?;
        assert_eq!(illegal["details"]["legal_actions"],legal["legal_actions"]);
        assert!(illegal["hint"].is_string());
        assert_eq!(invoke(&adapter,"get_state",json!({"match_id":id}),false).await?,initial);
        let request = json!({"match_id":id,"action":action,"turn":0,"idempotency_key":"move-one","reasoning":"choose a listed legal move"});
        let moved = invoke(&adapter,"make_move",request.clone(),false).await?;
        assert_eq!(moved["state"]["turn"],2);
        assert_eq!(moved["opponent_actions"].as_array().map(Vec::len),Some(1));
        assert_eq!(invoke(&adapter,"make_move",request,false).await?,moved);
        let stale = invoke(&adapter,"make_move",json!({"match_id":id,"action":action,"turn":0}),true).await?;
        assert_eq!(stale["code"],"STALE_TURN");
        let replay = app.service.get_replay(id,Viewer::Player(0)).await?;
        assert!(serde_json::to_string(&replay)?.contains("choose a listed legal move"));
        let final_state = invoke(&adapter,"resign",json!({"match_id":id}),false).await?;
        assert_eq!(final_state["terminated"],true);
        assert_eq!(final_state["outcome"]["reason"],"resigned");
    }
    let solo = invoke(&adapter,"create_match",json!({"game_id":"sudoku","seed":1}),false).await?;
    assert_eq!(solo["state"]["you"],0);
    let id = solo["state"]["match_id"].as_str().ok_or("id")?;
    invoke(&adapter,"make_move",json!({"match_id":id,"action":solo["state"]["legal_actions"][0]}),false).await?;
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn mcp_controls_respect_viewer_and_opponent_openings() -> Result<(),ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let black = McpServer::new(app.service.clone(),Viewer::Player(1))?;
    let created = invoke(&black,"create_match",json!({"game_id":"chess","opponent":"random","play_as":1,"seed":7}),false).await?;
    assert_eq!(created["state"]["turn"],1);
    assert_eq!(created["state"]["to_act"],json!([1]));
    let id = created["state"]["match_id"].as_str().ok_or("id")?;
    let before = app.service.get_state(id,Viewer::Player(1)).await?;
    let spectator = McpServer::new(app.service.clone(),Viewer::Spectator)?;
    for (name,args) in [
        ("create_match",json!({"game_id":"chess"})),
        ("make_move",json!({"match_id":id,"action":"e7e5"})),
        ("resign",json!({"match_id":id})),
    ] { assert_eq!(invoke(&spectator,name,args,true).await?["code"],"FORBIDDEN"); }
    assert_eq!(invoke(&black,"create_match",json!({"game_id":"chess","play_as":0}),true).await?["code"],"FORBIDDEN");
    assert_eq!(invoke(&black,"make_move",json!({"match_id":id,"action":"e7e5","seat":0}),true).await?["code"],"INVALID_REQUEST");
    assert_eq!(invoke(&black,"make_move",json!({"match_id":id,"action":"e7e5","idempotency_key":"missing-turn"}),true).await?["code"],"INVALID_REQUEST");
    assert_eq!(app.service.get_state(id,Viewer::Player(1)).await?,before);
    assert_eq!(invoke(&spectator,"get_state",json!({"match_id":id}),false).await?["legal_actions"],json!([]));
    app.store.close().await;
    Ok(())
}

use super::{mcp_play_tests::invoke,tests::{config,fixture},*};
use gfa_core::Viewer;
use gfa_mcp::McpServer;
use serde_json::{json,Value};
use std::collections::BTreeSet;

#[tokio::test]
async fn mcp_history_pages_and_replay_keep_reasoning_visibility() -> Result<(),ServerError> {
    let directory = tempfile::tempdir()?;
    let cfg = config(&directory);
    let app = fixture(&cfg).await?;
    let player = McpServer::new(app.service.clone(),Viewer::Player(0))?;
    let other = McpServer::new(app.service.clone(),Viewer::Player(1))?;
    let spectator = McpServer::new(app.service.clone(),Viewer::Spectator)?;
    let mut expected = BTreeSet::new();
    let mut first_id = String::new();
    for index in 0..3 {
        let game = if index == 2 {"chess"} else {"tictactoe"};
        let created = invoke(&player,"create_match",json!({"game_id":game,"seed":index}),false).await?;
        let id = created["state"]["match_id"].as_str().ok_or("id")?.to_string();
        if index == 0 { first_id=id.clone(); }
        if game == "tictactoe" { expected.insert(id); }
    }
    let mut cursor = Value::Null;
    let mut seen = BTreeSet::new();
    loop {
        let page = invoke(&player,"get_match_history",json!({"game_id":"tictactoe","limit":1,"after":cursor}),false).await?;
        for entry in page["matches"].as_array().ok_or("matches")? {
            assert!(seen.insert(entry["match_id"].as_str().ok_or("id")?.to_string()));
            assert!(entry.get("seed").is_none());
        }
        cursor=page["next"].clone();
        if cursor.is_null() { break; }
    }
    assert_eq!(seen,expected);
    invoke(&player,"make_move",json!({"match_id":first_id,"action":"r1c1","reasoning":"private active reasoning"}),false).await?;
    let own = invoke(&player,"get_replay",json!({"match_id":first_id}),false).await?;
    assert_eq!(own["events"][1]["reasoning"],"private active reasoning");
    for viewer in [&other,&spectator] {
        let hidden = invoke(viewer,"get_replay",json!({"match_id":first_id}),false).await?;
        assert!(hidden["events"][1]["reasoning"].is_null());
        assert!(!serde_json::to_string(&hidden)?.contains("private active reasoning"));
        assert!(hidden["events"][0]["details"].get("seed").is_none());
        assert!(hidden["events"][0]["details"].get("initial_state").is_none());
    }
    let one = invoke(&player,"get_replay",json!({"match_id":first_id,"limit":1}),false).await?;
    let two = invoke(&player,"get_replay",json!({"match_id":first_id,"limit":1,"since":one["next"]}),false).await?;
    assert_eq!(one["events"][0]["sequence"],0);
    assert_eq!(two["events"][0]["sequence"],1);
    assert_eq!(one["revision"],two["revision"]);
    assert!(two["next"].is_null());
    invoke(&other,"resign",json!({"match_id":first_id}),false).await?;
    let finished = invoke(&spectator,"get_replay",json!({"match_id":first_id}),false).await?;
    assert_eq!(finished["events"][1]["reasoning"],"private active reasoning");
    for (name,args) in [
        ("get_match_history",json!({"limit":0})),
        ("get_replay",json!({"match_id":first_id,"since":999})),
        ("get_replay",json!({"match_id":first_id,"limit":101})),
    ] { invoke(&player,name,args,true).await?; }
    assert_eq!(invoke(&spectator,"get_replay",json!({"match_id":first_id,"seat":0}),true).await?["code"],"INVALID_REQUEST");
    app.store.close().await;
    let reopened = fixture(&cfg).await?;
    let viewer = McpServer::new(reopened.service.clone(),Viewer::Spectator)?;
    assert_eq!(invoke(&viewer,"get_replay",json!({"match_id":first_id}),false).await?,finished);
    reopened.store.close().await;
    Ok(())
}

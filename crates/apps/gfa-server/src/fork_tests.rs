use super::tests::{call, config, fixture};
use axum::http::StatusCode;
use serde_json::{json, Value};

type TestResult = Result<(), super::ServerError>;

#[tokio::test]
async fn variations_keep_independent_moves_and_rebuild_parent_history_after_restart() -> TestResult {
    let directory = tempfile::tempdir()?;
    let cfg = config(&directory);
    let app = fixture(&cfg).await?;
    let (_, root) = call(&app.router, "POST", "/v1/matches", json!({"game_id":"tictactoe"}), None).await?;
    let root_id = root["match_id"].as_str().ok_or("root id")?;
    call(&app.router, "POST", &format!("/v1/matches/{root_id}/actions"),
        json!({"seat":0,"turn":0,"action":"r1c1"}), None).await?;
    let (status, fork) = call(&app.router, "POST", &format!("/v1/matches/{root_id}/fork?seat=1"),
        json!({"turn":1,"seed":42}), None).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(fork["turn"], 0);
    assert_eq!(fork["to_act"], json!([1]));
    assert!(fork["info"].is_object());
    let child_id = fork["match_id"].as_str().ok_or("child id")?;
    assert_ne!(root_id, child_id);
    let (_, metadata) = call(&app.router, "GET", &format!("/v1/matches/{child_id}"), Value::Null, None).await?;
    assert_eq!(metadata["forked_from"], json!({"match_id":root_id,"turn":1}));
    call(&app.router, "POST", &format!("/v1/matches/{child_id}/actions"),
        json!({"seat":1,"turn":0,"action":"r2c2"}), None).await?;
    let (_, root_after) = call(&app.router, "GET", &format!("/v1/matches/{root_id}/state?seat=0"), Value::Null, None).await?;
    assert_eq!(root_after["turn"], 1);
    assert_eq!(root_after["observation"]["json"]["board"][4], Value::Null);
    let (status, grandchild) = call(&app.router, "POST", &format!("/v1/matches/{child_id}/fork"),
        json!({"turn":1,"keep_rng":true,"include_info":false}), None).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert!(grandchild.get("info").is_none());
    let grandchild_id = grandchild["match_id"].as_str().ok_or("grandchild")?;
    let (_, replay) = call(&app.router, "GET", &format!("/v1/matches/{grandchild_id}/replay?seat=0"), Value::Null, None).await?;
    let ancestors = replay["ancestors"].as_array().ok_or("ancestors")?;
    assert_eq!(ancestors.len(), 2);
    assert_eq!(ancestors[0]["source"]["match_id"], root_id);
    assert_eq!(ancestors[1]["source"]["match_id"], child_id);
    assert_eq!(ancestors[0]["states"].as_array().ok_or("states")?.len(), 2);
    assert_eq!(ancestors[1]["states"].as_array().ok_or("states")?.len(), 2);
    assert_eq!(ancestors[0]["states"][0]["legal_actions"], json!([]));
    assert_eq!(replay["states"][0]["observation"], grandchild["observation"]);
    // Even a later control at the same turn cannot rewrite the fork's history.
    let (status, _) = call(&app.router, "POST", &format!("/v1/matches/{root_id}/resign"),
        json!({"seat":1,"turn":1}), None).await?;
    assert_eq!(status, StatusCode::OK);
    app.store.close().await;
    let reopened = fixture(&cfg).await?;
    assert_eq!(call(&reopened.router, "GET", &format!("/v1/matches/{grandchild_id}/replay?seat=0"), Value::Null, None).await?.1, replay);
    reopened.store.close().await;
    Ok(())
}

#[tokio::test]
async fn forks_validate_turn_rng_options_and_terminal_positions() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let (_, root) = call(&app.router, "POST", "/v1/matches", json!({"game_id":"tictactoe"}), None).await?;
    let id = root["match_id"].as_str().ok_or("id")?;
    for request in [json!({"turn":99}), json!({"turn":0,"keep_rng":true,"seed":42}), json!({"turn":0,"owns_parent":true})] {
        assert!(call(&app.router, "POST", &format!("/v1/matches/{id}/fork"), request, None).await?.0.is_client_error());
    }
    for (turn, action) in ["r1c1","r2c1","r1c2","r2c2","r1c3"].into_iter().enumerate() {
        call(&app.router, "POST", &format!("/v1/matches/{id}/actions"),
            json!({"seat":turn%2,"turn":turn,"action":action}), None).await?;
    }
    assert!(call(&app.router, "POST", &format!("/v1/matches/{id}/fork"), json!({"turn":5}), None).await?.0.is_client_error());
    assert_eq!(call(&app.router, "POST", &format!("/v1/matches/{id}/fork"), json!({"turn":3}), None).await?.0, StatusCode::CREATED);
    app.store.close().await;
    Ok(())
}

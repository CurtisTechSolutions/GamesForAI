use super::{
    tests::{call, config, fixture},
    *,
};
use axum::http::StatusCode;
use serde_json::{json, Value};

type TestResult = Result<(), ServerError>;

#[tokio::test]
async fn chess_draw_history_forks_and_replay_survive_restart() -> TestResult {
    let dir = tempfile::tempdir()?;
    let settings = config(&dir);
    let app = fixture(&settings).await?;
    let (status, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"chess","include_info":false,"seed":11}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(initial["legal_actions"].as_array().map(Vec::len), Some(20));
    assert_eq!(initial["action_mask"].as_array().map(Vec::len), Some(9345));
    let id = initial["match_id"].as_str().ok_or("id")?;
    let path = format!("/v1/matches/{id}/actions");
    for (turn, action) in [
        "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
    ]
    .iter()
    .enumerate()
    {
        let (status, _) = call(
            &app.router,
            "POST",
            &path,
            json!({"seat":turn%2,"turn":turn,"action":action}),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
    }
    let (_, child) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/fork?seat=0"),
        json!({"turn":8,"include_info":false}),
        None,
    )
    .await?;
    let child_id = child["state"]["match_id"].as_str().ok_or("child id")?;
    assert_eq!(child["state"]["turn"], 0);
    assert_eq!(child["state"]["observation"]["json"]["repetitions"], 3);
    let (_, claimed) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{child_id}/actions"),
        json!({"turn":0,"seat":0,"action":"claim_draw"}),
        Some("claim"),
    )
    .await?;
    assert_eq!(claimed["state"]["terminated"], true);
    assert_eq!(claimed["state"]["returns"], json!([0.0, 0.0]));
    let replay_path = format!("/v1/matches/{child_id}/replay?seat=0");
    let (status, replay) = call(&app.router, "GET", &replay_path, Value::Null, None).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["states"][0]["observation"]["json"]["repetitions"], 3);
    assert_eq!(replay["ancestors"].as_array().map(Vec::len), Some(1));
    assert_eq!(replay["states"][1], claimed["state"]);
    app.store.close().await;
    let reopened = fixture(&settings).await?;
    assert_eq!(
        call(&reopened.router, "GET", &replay_path, Value::Null, None)
            .await?
            .1,
        replay
    );
    reopened.store.close().await;
    Ok(())
}

#[tokio::test]
async fn chess_fen_validation_and_action_encodings_use_generic_routes() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    let position = "4k3/P7/8/8/8/8/7p/4K3 w - - 0 1";
    let (status, validated) = call(
        &app.router,
        "POST",
        "/v1/games/chess/positions/validate?seat=0",
        json!({"start":{"position":position}}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(validated["observation"]["json"]["fen"], position);
    let (status, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"chess","start":{"position":position},"include_info":false}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let id = initial["match_id"].as_str().ok_or("id")?;
    let promotion = initial["legal_actions"]
        .as_array()
        .ok_or("legal")?
        .iter()
        .find(|a| a["string"] == "a7a8n")
        .ok_or("underpromotion")?;
    let (status, played) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/actions"),
        json!({"seat":0,"turn":0,"action":{"index":promotion["index"]}}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(played["accepted_action"], *promotion);
    assert_eq!(played["state"]["observation"]["json"]["pieces"]["a8"], "N");
    let (status, error) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"chess","start":{"position":"8/8/8/8/8/8/8/4K3 w - - 0 1"}}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["code"], "INVALID_POSITION");
    app.store.close().await;
    Ok(())
}

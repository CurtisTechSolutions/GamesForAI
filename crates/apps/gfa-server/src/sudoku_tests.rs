use super::tests::{call, config, fixture};
use axum::http::StatusCode;
use serde_json::{json, Value};

type TestResult = Result<(), super::ServerError>;

#[tokio::test]
async fn single_player_notes_simulation_finish_and_restart_replay_exactly() -> TestResult {
    let directory = tempfile::tempdir()?;
    let cfg = config(&directory);
    let app = fixture(&cfg).await?;
    let game_config = json!({"size":4,"puzzle":"123434122143432.","mistake_policy":"silent"});
    let (status, created) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"sudoku","config":game_config,"seed":7}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["to_act"], json!([0]));
    let id = created["match_id"].as_str().ok_or("id")?;
    for (turn, action) in ["r4c4+1", "r4c4=2", "r4c4=0"].into_iter().enumerate() {
        let (status, moved) = call(
            &app.router,
            "POST",
            &format!("/v1/matches/{id}/actions"),
            json!({"seat":0,"turn":turn,"action":action}),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        if turn == 0 {
            assert_eq!(moved["state"]["observation"]["json"]["moves"], 0);
        }
        assert_eq!(
            moved["state"]["observation"]["json"]["mistakes"],
            if turn == 1 { json!(1) } else { Value::Null }
        );
    }
    let (_, simulated) = call(
        &app.router,
        "POST",
        "/v1/games/sudoku/simulate",
        json!({"from":{"match_id":id,"seat":0},"seed":99,"lines":[["r4c4=1"]]}),
        None,
    )
    .await?;
    let (status, moved) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/actions"),
        json!({"seat":0,"turn":3,"action":"r4c4=1"}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["state"]["terminated"], true);
    assert_eq!(moved["state"]["returns"], json!([1.0]));
    assert_eq!(moved["state"]["observation"]["json"]["mistakes"], 1);
    let mut expected = moved["state"].clone();
    expected["match_id"] = json!("");
    assert_eq!(simulated["lines"][0]["states"][0], expected);
    let (_, replay) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{id}/replay?seat=0"),
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(replay["states"].as_array().ok_or("states")?.len(), 5);
    assert!(!replay.to_string().contains("solution"));
    app.store.close().await;
    let reopened = fixture(&cfg).await?;
    assert_eq!(
        call(
            &reopened.router,
            "GET",
            &format!("/v1/matches/{id}/replay?seat=0"),
            Value::Null,
            None
        )
        .await?
        .1,
        replay
    );
    reopened.store.close().await;
    Ok(())
}

#[tokio::test]
async fn validation_and_custom_start_briefings_do_not_disclose_solutions() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let cfg = json!({"size":4});
    let (status, validated) = call(
        &app.router,
        "POST",
        "/v1/games/sudoku/positions/validate",
        json!({"config":cfg,"start":{"position":"123434122143432."},"seed":9}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(validated["state"].get("solution").is_none());
    assert!(validated["state"].get("validated_puzzle").is_none());
    let (status, created) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"sudoku","config":cfg,"start":{"state":validated["state"]}}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let live = &created["info"]["sections"][0]["data"];
    assert!(live["start"].get("position").is_none());
    assert_eq!(live["start"]["view"], created["observation"]);
    let id = created["match_id"].as_str().ok_or("id")?;
    let (status, _) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{id}/state?seat=1"),
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    app.store.close().await;
    Ok(())
}

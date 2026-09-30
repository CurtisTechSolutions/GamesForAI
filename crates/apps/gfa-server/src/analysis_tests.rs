use super::{
    tests::{call, config, fixture},
    *,
};
use axum::http::StatusCode;
use serde_json::{json, Value};

#[tokio::test]
async fn real_analysis_finds_a_win_without_mutating_history_and_reports_uncalibrated_levels(
) -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let (status, opponents) = call(
        &app.router,
        "GET",
        "/v1/games/tictactoe/opponents",
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        opponents
            .as_array()
            .ok_or("catalog")?
            .iter()
            .map(|p| p["id"].as_str().unwrap_or(""))
            .collect::<Vec<_>>(),
        ["mcts", "minimax", "random"]
    );
    for player in opponents.as_array().ok_or("catalog")? {
        assert_eq!(player["calibrated"], false);
        for level in player["levels"].as_array().ok_or("levels")? {
            assert!(level["rating"].is_null());
        }
    }
    let (_, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({
            "game_id":"tictactoe","seed":42,"assists":{"allow_analysis":true}
        }),
        None,
    )
    .await?;
    let id = initial["match_id"].as_str().ok_or("match id")?;
    for (turn, action) in ["r1c1", "r2c1", "r1c2", "r2c2"].iter().enumerate() {
        let (status, _) = call(
            &app.router,
            "POST",
            &format!("/v1/matches/{id}/actions"),
            json!({
                "seat":turn%2,"turn":turn,"action":action
            }),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
    }
    let (_, before) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{id}/replay?seat=0"),
        Value::Null,
        None,
    )
    .await?;
    let body = json!({
        "game_id":"tictactoe","from":{"match_id":id,"seat":0},
        "opponent":{"id":"minimax","level":3,"limits":{"nodes":512,"time_ms":5000}},
        "seed":17
    });
    let (status, result) = call(&app.router, "POST", "/v1/analysis", body.clone(), None).await?;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["best_moves"][0]["string"], "r1c3");
    assert_eq!(result["evaluation"], 1.0);
    assert!(result["nodes"].as_u64().ok_or("nodes")? <= 512);
    let (_, again) = call(&app.router, "POST", "/v1/analysis", body.clone(), None).await?;
    assert_eq!(again, result);
    let (_, after) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{id}/replay?seat=0"),
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(before, after);
    let (status, error) = call(
        &app.router,
        "POST",
        "/v1/analysis?seat=1",
        body.clone(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "ASSIST_NOT_ALLOWED");
    let mut request = body;
    request["opponent"]["limits"]["nodes"] = json!(1_000_001);
    let (status, _) = call(&app.router, "POST", "/v1/analysis", request, None).await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &app.router,
        "GET",
        "/v1/games/missing/opponents",
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn sudoku_reference_solver_plays_only_through_generic_analysis_and_moves(
) -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let (_, catalog) = call(
        &app.router,
        "GET",
        "/v1/games/sudoku/opponents",
        Value::Null,
        None,
    )
    .await?;
    assert!(catalog
        .as_array()
        .ok_or("catalog")?
        .iter()
        .any(|item| item["id"] == "reference"));
    let (_, mut state) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({
            "game_id":"sudoku", "config":{"size":4}, "seed":42,
            "assists":{"allow_analysis":true}
        }),
        None,
    )
    .await?;
    let id = state["match_id"].as_str().ok_or("id")?.to_owned();
    for _ in 0..16 {
        if state["terminated"] == true {
            break;
        }
        let (status, result) = call(
            &app.router,
            "POST",
            "/v1/analysis",
            json!({
                "game_id":"sudoku","from":{"match_id":id,"seat":0},
                "opponent":{"id":"reference"},"seed":7
            }),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert!(result["advice"]["summary"]
            .as_str()
            .is_some_and(|s| !s.is_empty()));
        assert!(state["legal_actions"]
            .as_array()
            .ok_or("legal")?
            .contains(&result["best_moves"][0]));
        let (status, moved) = call(
            &app.router,
            "POST",
            &format!("/v1/matches/{id}/actions"),
            json!({
                "seat":0,"turn":state["turn"],"action":result["best_moves"][0]["json"]
            }),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        state = moved["state"].clone();
    }
    assert_eq!(state["returns"], json!([1.0]));
    assert_eq!(state["observation"]["json"]["outcome"], "solved");
    app.store.close().await;
    Ok(())
}

use super::{
    tests::{call, config, fixture},
    *,
};
use axum::http::StatusCode;
use serde_json::{json, Value};

#[tokio::test]
async fn configured_missing_stockfish_fails_before_database_creation() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let mut config = config(&directory);
    config.stockfish = Some(StockfishConfig::linux("/does-not-exist-gfa-stockfish"));
    assert!(fixture(&config).await.is_err());
    assert!(!directory.path().join("matches.sqlite").exists());
    Ok(())
}

#[tokio::test]
#[ignore = "requires the installed Stockfish sandbox in UCI CI"]
async fn stockfish_http_seats_analysis_and_idempotency() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let mut config = config(&directory);
    config.stockfish = Some(StockfishConfig::linux("/usr/games/stockfish"));
    let app = fixture(&config).await?;
    let (status, catalog) = call(
        &app.router,
        "GET",
        "/v1/games/chess/opponents",
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let stockfish = catalog
        .as_array()
        .ok_or("catalog")?
        .iter()
        .find(|item| item["id"] == "stockfish")
        .ok_or("stockfish absent")?;
    assert_eq!(stockfish["calibrated"], false);
    assert_eq!(stockfish["levels"].as_array().ok_or("levels")?.len(), 10);
    assert!(stockfish["levels"]
        .as_array()
        .ok_or("levels")?
        .iter()
        .all(|item| item["rating"].is_null()));
    let (_, other) = call(
        &app.router,
        "GET",
        "/v1/games/sudoku/opponents",
        Value::Null,
        None,
    )
    .await?;
    assert!(!other
        .as_array()
        .ok_or("catalog")?
        .iter()
        .any(|item| item["id"] == "stockfish"));
    let (status, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({
            "game_id":"chess","seed":7,"assists":{"allow_analysis":true},
            "seats":[{"type":"self"},{"type":"opponent","opponent":{
                "id":"stockfish","level":1,"limits":{"nodes":500,"depth":3,"time_ms":5000}
            }}]
        }),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{initial}");
    let id = initial["match_id"].as_str().ok_or("id")?;
    let request = json!({"seat":0,"turn":0,"action":"e2e4"});
    let path = format!("/v1/matches/{id}/actions");
    let (status, moved) = call(
        &app.router,
        "POST",
        &path,
        request.clone(),
        Some("stockfish-first"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["state"]["turn"], 2);
    assert_eq!(
        moved["opponent_actions"].as_array().ok_or("replies")?.len(),
        1
    );
    let (_, retried) = call(&app.router, "POST", &path, request, Some("stockfish-first")).await?;
    assert_eq!(moved, retried);
    let replay_path = format!("/v1/matches/{id}/replay?seat=0");
    let (_, before) = call(&app.router, "GET", &replay_path, Value::Null, None).await?;
    let (status, analysis) = call(&app.router, "POST", "/v1/analysis", json!({
        "game_id":"chess","from":{"match_id":id,"seat":0},
        "opponent":{"id":"stockfish","uci":{"elo":1500,"multipv":2},"limits":{"nodes":2000,"depth":4,"time_ms":5000}}
    }), None).await?;
    assert_eq!(status, StatusCode::OK, "{analysis}");
    assert!(moved["state"]["legal_actions"]
        .as_array()
        .ok_or("legal")?
        .contains(&analysis["best_moves"][0]));
    assert!(analysis["evaluation"].is_null());
    assert_eq!(
        analysis["variations"]
            .as_array()
            .ok_or("variations")?
            .len(),
        2
    );
    let (_, after) = call(&app.router, "GET", &replay_path, Value::Null, None).await?;
    assert_eq!(before, after);
    let (status, error) = call(
        &app.router,
        "POST",
        "/v1/analysis",
        json!({
            "game_id":"chess","from":{"match_id":id,"seat":0},
            "opponent":{"id":"stockfish","uci":{"elo":1}}
        }),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["code"], "INVALID_CONFIG");
    app.store.close().await;
    Ok(())
}

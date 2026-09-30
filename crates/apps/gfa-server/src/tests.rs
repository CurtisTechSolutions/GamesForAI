use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;

type TestResult = Result<(), ServerError>;

pub(super) async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Value,
    key: Option<&str>,
) -> Result<(StatusCode, Value), ServerError> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:8080")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    let mut request = request.body(if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    })?;
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))));
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok((status, serde_json::from_slice(&body)?))
}
fn match_id(body: &Value) -> Result<String, ServerError> {
    body["match_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "missing match ID".into())
}
pub(super) fn config(directory: &tempfile::TempDir) -> Config {
    Config {
        sqlite: directory.path().join("matches.sqlite"),
        port: 8080,
    }
}
pub(super) async fn fixture(config: &Config) -> Result<Application, ServerError> {
    application(config, SocketAddr::from(([127, 0, 0, 1], 8080))).await
}

#[tokio::test]
async fn tic_tac_toe_lifecycle_replay_and_retry_survive_restart() -> TestResult {
    let dir = tempfile::tempdir()?;
    let config = config(&dir);
    let app = fixture(&config).await?;
    let (status, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"tictactoe","include_info":false,"seed":42}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(initial["legal_actions"].as_array().map(Vec::len), Some(9));
    let id = match_id(&initial)?;
    let actions = format!("/v1/matches/{id}/actions");
    let first_request = json!({"seat":0,"turn":0,"action":"r1c1","reasoning":"take a corner"});
    let (status, first) = call(
        &app.router,
        "POST",
        &actions,
        first_request.clone(),
        Some("first"),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    for (turn, action) in [
        (1, json!({"row":2,"col":1})),
        (2, json!({"index":1})),
        (3, json!("r2c2")),
        (4, json!("r1c3")),
    ] {
        let (status, _) = call(
            &app.router,
            "POST",
            &actions,
            json!({"seat":turn%2,"turn":turn,"action":action}),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
    }
    let state_path = format!("/v1/matches/{id}/state?seat=0");
    let (_, final_state) = call(&app.router, "GET", &state_path, Value::Null, None).await?;
    assert_eq!(final_state["turn"], 5);
    assert_eq!(final_state["terminated"], true);
    assert_eq!(final_state["returns"], json!([1.0, -1.0]));
    let replay_path = format!("/v1/matches/{id}/replay?seat=0");
    let (_, replay) = call(&app.router, "GET", &replay_path, Value::Null, None).await?;
    assert_eq!(replay["states"].as_array().map(Vec::len), Some(6));
    assert_eq!(replay["states"][5], final_state);
    app.store.close().await;
    let reopened = fixture(&config).await?;
    assert_eq!(
        call(&reopened.router, "GET", &state_path, Value::Null, None)
            .await?
            .1,
        final_state
    );
    assert_eq!(
        call(&reopened.router, "GET", &replay_path, Value::Null, None)
            .await?
            .1,
        replay
    );
    assert_eq!(
        call(
            &reopened.router,
            "POST",
            &actions,
            first_request,
            Some("first")
        )
        .await?
        .1,
        first
    );
    let (status, error) = call(
        &reopened.router,
        "POST",
        &actions,
        json!({"seat":0,"turn":5,"action":"r3c3"}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "MATCH_FINISHED");
    reopened.store.close().await;
    Ok(())
}

#[tokio::test]
async fn invalid_and_racing_moves_preserve_the_committed_state() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    let (_, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"tictactoe","include_info":false}),
        None,
    )
    .await?;
    let id = match_id(&initial)?;
    let actions = format!("/v1/matches/{id}/actions");
    for (request, expected, status) in [
        (
            json!({"seat":1,"turn":0,"action":"r1c1"}),
            "NOT_YOUR_TURN",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"seat":0,"turn":1,"action":"r1c1"}),
            "STALE_TURN",
            StatusCode::CONFLICT,
        ),
        (
            json!({"seat":0,"turn":0,"action":"invalid"}),
            "UNPARSEABLE_ACTION",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (actual, error) = call(&app.router, "POST", &actions, request, None).await?;
        assert_eq!(actual, status);
        assert_eq!(error["error"]["code"], expected);
    }
    let state_path = format!("/v1/matches/{id}/state?seat=0");
    assert_eq!(
        call(&app.router, "GET", &state_path, Value::Null, None)
            .await?
            .1,
        initial
    );
    let (a, b) = tokio::join!(
        call(
            &app.router,
            "POST",
            &actions,
            json!({"seat":0,"turn":0,"action":"r1c1"}),
            Some("a")
        ),
        call(
            &app.router,
            "POST",
            &actions,
            json!({"seat":0,"turn":0,"action":"r1c2"}),
            Some("b")
        ),
    );
    let statuses = [a?.0, b?.0];
    assert_eq!(
        statuses
            .iter()
            .filter(|&&status| status == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|&&status| status == StatusCode::CONFLICT)
            .count(),
        1
    );
    assert_eq!(
        call(&app.router, "GET", &state_path, Value::Null, None)
            .await?
            .1["turn"],
        1
    );
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn connect_four_uses_the_same_routes_and_spectator_projection() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    let (_, games) = call(&app.router, "GET", "/v1/games", Value::Null, None).await?;
    assert_eq!(games.as_array().map(Vec::len), Some(2));
    let (status, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"connect4","include_info":false,"seed":7}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let id = match_id(&initial)?;
    let (status, result) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/actions"),
        json!({"seat":0,"turn":0,"action":"4"}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["accepted_action"]["index"], 3);
    let (_, public) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{id}/state"),
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(public["turn"], 1);
    assert_eq!(public["legal_actions"], json!([]));
    let (_, legal) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{id}/legal-actions?seat=1"),
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(legal["legal_actions"].as_array().map(Vec::len), Some(7));
    assert_eq!(
        legal["action_mask"],
        json!([true, true, true, true, true, true, true])
    );
    app.store.close().await;
    Ok(())
}

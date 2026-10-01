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
        database: Database::Sqlite(directory.path().join("matches.sqlite")),
        port: 8080,
        stockfish: None,
        mcp_seat: Some(0),
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
    assert_eq!(games.as_array().map(Vec::len), Some(4));
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

#[tokio::test]
async fn position_validation_accepts_finished_boards_and_round_trips_all_encodings() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    for (game, position, terminal) in [
        (
            "tictactoe",
            json!({"board":[0,0,0,1,1,null,null,null,null],"to_move":1}),
            true,
        ),
        ("connect4", json!({"boards":[1,0],"to_move":1}), false),
    ] {
        let path = format!("/v1/games/{game}/positions/validate?seat=1");
        let (status, result) = call(
            &app.router,
            "POST",
            &path,
            json!({"start":{"state":position},"seed":7}),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["state"], position);
        assert_eq!(result["terminated"], terminal);
        let (status, restored) = call(
            &app.router,
            "POST",
            &path,
            json!({"start":{"position":result["position"]},"seed":7}),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result, restored);
        if terminal {
            assert_eq!(result["to_act"], json!([]));
            assert_eq!(result["legal_actions"], json!([]));
            assert_eq!(result["returns"], json!([1.0, -1.0]));
            let (status, error) = call(
                &app.router,
                "POST",
                "/v1/matches",
                json!({"game_id":game,"start":{"state":position}}),
                None,
            )
            .await?;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(error["error"]["code"], "INVALID_POSITION");
        } else {
            assert_eq!(result["legal_actions"].as_array().map(Vec::len), Some(7));
        }
    }
    for (game, request, code) in [
        (
            "connect4",
            json!({"start":{"state":{"boards":[2,0],"to_move":1}}}),
            "INVALID_POSITION",
        ),
        (
            "tictactoe",
            json!({"start":{"position":"not a board"}}),
            "INVALID_POSITION",
        ),
        (
            "tictactoe",
            json!({"config":{"bad":true},"start":{"state":{}}}),
            "INVALID_CONFIG",
        ),
        ("missing", json!({"start":{"state":{}}}), "UNKNOWN_GAME"),
    ] {
        let (_, error) = call(
            &app.router,
            "POST",
            &format!("/v1/games/{game}/positions/validate"),
            request,
            None,
        )
        .await?;
        assert_eq!(error["error"]["code"], code);
    }
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn metadata_and_history_paginate_filter_and_survive_restart() -> TestResult {
    let dir = tempfile::tempdir()?;
    let settings = config(&dir);
    let app = fixture(&settings).await?;
    let mut ids = Vec::new();
    for game_id in ["tictactoe", "connect4", "tictactoe"] {
        let (_, created) = call(
            &app.router,
            "POST",
            "/v1/matches",
            json!({"game_id":game_id,"seed":42}),
            None,
        )
        .await?;
        ids.push(match_id(&created)?);
    }
    for (turn, action) in ["r1c1", "r2c1", "r1c2", "r2c2", "r1c3"].iter().enumerate() {
        call(
            &app.router,
            "POST",
            &format!("/v1/matches/{}/actions", ids[0]),
            json!({"seat":turn%2,"turn":turn,"action":action}),
            None,
        )
        .await?;
    }
    let (status, meta) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{}", ids[0]),
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(meta["status"], "finished");
    assert_eq!(meta["turn"], 5);
    for private in ["seed", "start", "state", "reasoning"] {
        assert!(meta.get(private).is_none());
    }
    app.store.close().await;
    let app = fixture(&settings).await?;
    let mut cursor = String::new();
    let mut found = Vec::new();
    loop {
        let (_, page) = call(
            &app.router,
            "GET",
            &format!("/v1/matches?limit=1{cursor}"),
            Value::Null,
            None,
        )
        .await?;
        let records = page["matches"].as_array().ok_or("records")?;
        assert_eq!(records.len(), 1);
        found.push(records[0]["match_id"].as_str().ok_or("id")?.to_owned());
        match page["next"].as_str() {
            Some(next) => cursor = format!("&after={next}"),
            None => break,
        }
        assert!(found.len() <= 3);
    }
    ids.sort();
    assert_eq!(found, ids);
    let (_, finished) = call(
        &app.router,
        "GET",
        "/v1/matches?game_id=tictactoe&status=finished",
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(finished["matches"].as_array().ok_or("matches")?.len(), 1);
    let (_, active) = call(
        &app.router,
        "GET",
        "/v1/matches?status=active",
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(active["matches"].as_array().ok_or("matches")?.len(), 2);
    for path in [
        "/v1/matches?limit=0",
        "/v1/matches?limit=101",
        "/v1/matches?status=unknown",
        "/v1/matches?typo=1",
        "/v1/matches?limit=1&limit=2",
        "/v1/matches?game_id=missing",
    ] {
        let (status, error) = call(&app.router, "GET", path, Value::Null, None).await?;
        assert!(status.is_client_error(), "{path}");
        assert!(error["error"]["code"].is_string());
    }
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn resignation_and_draw_agreements_replay_after_restart() -> TestResult {
    let directory = tempfile::tempdir()?;
    let settings = config(&directory);
    let app = fixture(&settings).await?;
    let (_, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"tictactoe"}),
        None,
    )
    .await?;
    let id = match_id(&initial)?;
    let (status, resigned) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/resign"),
        json!({"seat":1,"turn":0}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resigned["turn"], 0);
    assert_eq!(resigned["observation"], initial["observation"]);
    assert_eq!(resigned["returns"], json!([1.0, -1.0]));
    assert_eq!(resigned["outcome"], json!({"reason":"resigned","seat":1}));
    assert_eq!(resigned["terminated"], true);
    assert_eq!(resigned["legal_actions"], json!([]));
    let (status, _) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/actions"),
        json!({"seat":0,"turn":0,"action":"r1c1"}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, created) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"connect4"}),
        None,
    )
    .await?;
    let drawn_id = match_id(&created)?;
    let offer_url = format!("/v1/matches/{drawn_id}/offer-draw");
    let (_, offered) = call(
        &app.router,
        "POST",
        &offer_url,
        json!({"seat":0,"turn":0}),
        None,
    )
    .await?;
    assert_eq!(offered["draw_offer"], 0);
    assert_eq!(offered["terminated"], false);
    let (_, duplicate) = call(
        &app.router,
        "POST",
        &offer_url,
        json!({"seat":0,"turn":0}),
        None,
    )
    .await?;
    assert_eq!(offered, duplicate);
    let (_, drawn) = call(
        &app.router,
        "POST",
        &offer_url,
        json!({"seat":1,"turn":0}),
        None,
    )
    .await?;
    assert_eq!(drawn["outcome"], json!({"reason":"agreed_draw"}));
    assert_eq!(drawn["returns"], json!([0.0, 0.0]));
    assert!(drawn.get("draw_offer").is_none());
    let (_, doc) = call(&app.router, "GET", "/v1/openapi.json", Value::Null, None).await?;
    assert!(doc["components"]["schemas"]["MatchOutcome"].is_object());
    app.store.close().await;
    let app = fixture(&settings).await?;
    for (id, expected) in [(id, resigned), (drawn_id, drawn)] {
        let (_, replay) = call(
            &app.router,
            "GET",
            &format!("/v1/matches/{id}/replay?seat=1"),
            Value::Null,
            None,
        )
        .await?;
        assert_eq!(replay["states"], json!([expected]));
        let (_, metadata) = call(
            &app.router,
            "GET",
            &format!("/v1/matches/{id}"),
            Value::Null,
            None,
        )
        .await?;
        assert_eq!(metadata["status"], "finished");
        assert_eq!(metadata["outcome"], expected["outcome"]);
    }
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn controls_validate_turns_expire_offers_and_commit_once_under_races() -> TestResult {
    use gfa_api_types::{ControlRequest, MoveRequest};
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let (_, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"tictactoe"}),
        None,
    )
    .await?;
    let id = match_id(&initial)?;
    for (request, expected) in [
        (json!({"seat":0,"turn":1}), StatusCode::CONFLICT),
        (json!({"seat":9,"turn":0}), StatusCode::UNPROCESSABLE_ENTITY),
        (json!({"seat":0}), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let (status, _) = call(
            &app.router,
            "POST",
            &format!("/v1/matches/{id}/resign"),
            request,
            None,
        )
        .await?;
        assert_eq!(status, expected);
    }
    app.service
        .offer_draw(&id, ControlRequest { seat: 0, turn: 0 })
        .await?;
    app.service
        .make_move(
            &id,
            MoveRequest {
                seat: 0,
                turn: 0,
                action: json!("r2c2"),
                reasoning: None,
            },
            None,
        )
        .await?;
    let offer = app
        .service
        .offer_draw(&id, ControlRequest { seat: 1, turn: 1 })
        .await?;
    assert!(!offer.terminated);
    assert_eq!(offer.draw_offer, Some(1));
    let (a, b) = tokio::join!(
        app.service.resign(&id, ControlRequest { seat: 0, turn: 1 }),
        app.service.resign(&id, ControlRequest { seat: 1, turn: 1 }),
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(a
        .err()
        .into_iter()
        .chain(b.err())
        .all(|error| error.code == "STALE_TURN" || error.code == "MATCH_FINISHED"));
    app.store.close().await;
    Ok(())
}

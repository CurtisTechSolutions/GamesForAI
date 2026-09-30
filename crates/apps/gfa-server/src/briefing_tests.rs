use super::{
    tests::{call, config, fixture},
    *,
};
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header, HeaderMap, Request, StatusCode},
};
use gfa_api_types::Briefing;
use serde_json::{json, Value};
use tower::ServiceExt;

type TestResult = Result<(), ServerError>;

async fn raw(
    router: &axum::Router,
    path: &str,
    conditional: Option<&str>,
) -> Result<(StatusCode, HeaderMap, Vec<u8>), ServerError> {
    let mut request = Request::builder()
        .uri(path)
        .header(header::HOST, "127.0.0.1:8080");
    if let Some(etag) = conditional {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    let mut request = request.body(Body::empty())?;
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50000))));
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 1024 * 1024).await?.to_vec();
    Ok((status, headers, body))
}

fn data<'a>(info: &'a Briefing, id: &str) -> Result<&'a Value, ServerError> {
    info.sections
        .iter()
        .find(|section| section.id == id)
        .map(|section| &section.data)
        .ok_or_else(|| format!("Missing section {id}").into())
}

#[tokio::test]
async fn every_game_briefing_contains_executable_examples_and_matching_views() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    for game in ["tictactoe", "connect4", "sudoku", "chess"] {
        let path = format!("/v1/games/{game}/info");
        let (status, _, body) = raw(&app.router, &path, None).await?;
        assert_eq!(status, StatusCode::OK);
        let info: Briefing = serde_json::from_slice(&body)?;
        let names: Vec<_> = info
            .sections
            .iter()
            .map(|section| section.id.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "identity",
                "players",
                "properties",
                "objective",
                "rules",
                "action_format",
                "observation_format",
                "initial_state",
                "example_turns",
                "rewards",
                "config_options",
                "time_controls",
                "opponents",
                "illegal_move_policy",
                "how_to_play",
                "strategy_notes",
                "limits"
            ]
        );
        assert!(info
            .sections
            .iter()
            .all(|section| !section.text.trim().is_empty()));
        let initial = call(
            &app.router,
            "POST",
            "/v1/matches",
            json!({"game_id":game,"seed":0}),
            None,
        )
        .await?
        .1;
        let id = initial["match_id"].as_str().ok_or("missing id")?;
        assert_preview_matches(data(&info, "initial_state")?, &initial)?;
        assert_eq!(
            data(&info, "initial_state")?["legal_actions"],
            initial["legal_actions"]
        );
        let examples = data(&info, "example_turns")?
            .as_array()
            .ok_or("missing examples")?;
        for example in examples
            .iter()
            .filter(|item| item.get("response").is_some())
        {
            let (status, _) = call(
                &app.router,
                "POST",
                &format!("/v1/matches/{id}/actions"),
                example["request"].clone(),
                None,
            )
            .await?;
            assert_eq!(status, StatusCode::OK);
            let (_, mut state) = call(
                &app.router,
                "GET",
                &format!("/v1/matches/{id}/state?seat=0"),
                Value::Null,
                None,
            )
            .await?;
            state
                .as_object_mut()
                .ok_or("state not object")?
                .remove("match_id");
            assert_preview_matches(&example["response"], &state)?;
        }
        let invalid = examples.last().ok_or("missing invalid example")?;
        assert_eq!(invalid["error"]["code"], "UNPARSEABLE_ACTION");
        let (status, error) = call(
            &app.router,
            "POST",
            "/v1/matches",
            json!({"game_id":game,"config":{"unknown":true}}),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "INVALID_CONFIG");
        let (_, _, compact) = raw(&app.router, &format!("{path}?detail=compact"), None).await?;
        let compact: Briefing = serde_json::from_slice(&compact)?;
        assert!(data(&compact, "action_format")?.get("schema").is_none());
        assert!(data(&compact, "observation_format")?
            .get("schema")
            .is_none());
        assert!(data(&compact, "config_options")?.get("schema").is_none());
        assert_eq!(
            data(&compact, "observation_format")?["tensor_shape"],
            if game == "tictactoe" {
                json!([3, 3, 3])
            } else if game == "chess" {
                json!([20, 8, 8])
            } else if game == "sudoku" {
                json!([10, 9, 9])
            } else {
                Value::Null
            }
        );
        assert!(data(&compact, "strategy_notes")?.is_null());
        assert!(compact.approx_tokens < info.approx_tokens);
        println!(
            "{game} compact briefing estimate: {}",
            compact.approx_tokens
        );
        assert!(
            compact.approx_tokens < 1500,
            "{game}: {} tokens",
            compact.approx_tokens
        );
    }
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn representations_are_equivalent_and_etags_track_query_variants() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    let path = "/v1/games/tictactoe/info";
    let (_, json_headers, json_body) = raw(&app.router, path, None).await?;
    let info: Briefing = serde_json::from_slice(&json_body)?;
    let (_, md_headers, md_body) =
        raw(&app.router, &format!("{path}?format=markdown"), None).await?;
    assert_eq!(String::from_utf8(md_body)?, info.markdown()?);
    assert_eq!(
        md_headers[header::CONTENT_TYPE],
        "text/markdown; charset=utf-8"
    );
    let etag = json_headers[header::ETAG].to_str()?;
    assert_ne!(md_headers[header::ETAG], etag);
    for conditional in [etag.to_string(), format!("\"other\", W/{etag}"), "*".into()] {
        let (status, headers, body) = raw(&app.router, path, Some(&conditional)).await?;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert!(body.is_empty());
        assert_eq!(headers[header::ETAG], etag);
    }
    let (_, default_headers, default_body) =
        raw(&app.router, &format!("{path}?config=%7B%7D"), None).await?;
    assert_eq!(default_body, json_body);
    assert_eq!(default_headers[header::ETAG], etag);
    for query in ["seat=1", "detail=compact", "format=markdown"] {
        let (status, headers, _) = raw(&app.router, &format!("{path}?{query}"), Some(etag)).await?;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(headers[header::ETAG], etag);
    }
    for (query, status) in [
        ("detail=unknown", StatusCode::BAD_REQUEST),
        ("format=html", StatusCode::BAD_REQUEST),
        ("config=bad", StatusCode::BAD_REQUEST),
        ("seat=bad", StatusCode::BAD_REQUEST),
        ("seat=255", StatusCode::UNPROCESSABLE_ENTITY),
        (
            "config=%7B%22unknown%22%3Atrue%7D",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        ("seat=0&seat=1", StatusCode::BAD_REQUEST),
        ("unexpected=true", StatusCode::BAD_REQUEST),
    ] {
        let (actual, _, body) = raw(&app.router, &format!("{path}?{query}"), None).await?;
        assert_eq!(actual, status);
        assert!(serde_json::from_slice::<Value>(&body)?["error"]["hint"].is_string());
    }
    assert_eq!(
        raw(&app.router, "/v1/games/missing/info", None).await?.0,
        StatusCode::NOT_FOUND
    );
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn match_briefings_cover_custom_starts_opt_out_and_restart() -> TestResult {
    let dir = tempfile::tempdir()?;
    let config = config(&dir);
    let app = fixture(&config).await?;
    let position = json!({"board":[0,null,null,null,1,null,null,null,null],"to_move":0});
    let (status, mut created) = call(
        &app.router,
        "POST",
        "/v1/matches?seat=1",
        json!({"game_id":"tictactoe","seed":1234,"start":{"state":position}}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let info: Briefing = serde_json::from_value(
        created
            .as_object_mut()
            .ok_or("creation object")?
            .remove("info")
            .ok_or("creation briefing")?,
    )?;
    let id = created["match_id"].as_str().ok_or("match id")?;
    let live = data(&info, "match")?;
    assert_eq!(live["state"], created);
    assert_eq!(live["you"]["seat"], 1);
    assert_eq!(live["status"], "active");
    assert_eq!(
        serde_json::from_str::<Value>(live["start"]["position"].as_str().ok_or("position")?)?,
        position
    );
    let path = format!("/v1/matches/{id}/info?seat=1&detail=compact");
    let (status, headers, body) = raw(&app.router, &path, Some("*")).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(!headers.contains_key(header::ETAG));
    assert_eq!(serde_json::from_slice::<Briefing>(&body)?, info);
    let (_, _, markdown) = raw(&app.router, &format!("{path}&format=markdown"), None).await?;
    assert_eq!(String::from_utf8(markdown)?, info.markdown()?);
    let (_, _, spectator) = raw(&app.router, &format!("/v1/matches/{id}/info"), None).await?;
    let spectator: Briefing = serde_json::from_slice(&spectator)?;
    assert!(data(&spectator, "match")?["you"].is_null());
    assert_eq!(
        data(&spectator, "match")?["state"]["legal_actions"],
        json!([])
    );
    assert_eq!(
        data(&spectator, "initial_state")?["legal_actions"],
        json!([])
    );
    let (status, _) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/actions"),
        json!({"seat":0,"turn":0,"action":"r1c2"}),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let (_, _, current) = raw(&app.router, &path, None).await?;
    let current_info: Briefing = serde_json::from_slice(&current)?;
    assert_eq!(data(&current_info, "match")?["turn"], 1);
    assert_eq!(data(&current_info, "match")?["start"], live["start"]);
    for (query, expected) in [
        ("seat=255", StatusCode::UNPROCESSABLE_ENTITY),
        ("config=%7B%7D", StatusCode::BAD_REQUEST),
        ("detail=unknown", StatusCode::BAD_REQUEST),
        ("format=html", StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(
            raw(&app.router, &format!("/v1/matches/{id}/info?{query}"), None)
                .await?
                .0,
            expected
        );
    }
    let (_, opted_out) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"tictactoe","include_info":false}),
        None,
    )
    .await?;
    assert!(opted_out.get("info").is_none());
    assert!(opted_out["observation"].is_object());
    assert_eq!(
        call(
            &app.router,
            "POST",
            "/v1/matches",
            json!({"game_id":"tictactoe","include_info":"false"}),
            None
        )
        .await?
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        raw(&app.router, "/v1/matches/missing/info", None).await?.0,
        StatusCode::NOT_FOUND
    );
    app.store.close().await;
    let reopened = fixture(&config).await?;
    assert_eq!(raw(&reopened.router, &path, None).await?.2, current);
    reopened.store.close().await;
    Ok(())
}

fn assert_preview_matches(preview: &Value, actual: &Value) -> Result<(), ServerError> {
    let mut expected = actual.clone();
    let object = expected.as_object_mut().ok_or("state")?;
    object.remove("match_id");
    object.remove("info");
    let mut preview = preview.clone();
    if let Some(omitted) = preview
        .as_object_mut()
        .ok_or("preview")?
        .remove("omitted_fields")
    {
        for field in omitted.as_array().ok_or("omitted fields")? {
            match field.as_str().ok_or("field")? {
                "action_mask" => {
                    object.remove("action_mask").ok_or("missing live mask")?;
                }
                "observation.tensor" => {
                    object
                        .get_mut("observation")
                        .and_then(Value::as_object_mut)
                        .ok_or("observation")?
                        .remove("tensor")
                        .ok_or("missing live tensor")?;
                }
                unknown => return Err(format!("Unexpected omitted field {unknown}").into()),
            }
        }
    }
    assert_eq!(preview, expected);
    Ok(())
}

#[tokio::test]
async fn chess_briefings_fit_prompt_budgets_without_changing_live_state() -> TestResult {
    let dir = tempfile::tempdir()?;
    let store = std::sync::Arc::new(
        gfa_store::SqliteMatchStore::open(dir.path().join("briefing.sqlite")).await?,
    );
    let mut registry = gfa_core::GameRegistry::default();
    registry.register::<gfa_game_chess::ChessGame>()?;
    let host = std::sync::Arc::new(Host);
    let service = gfa_service::GameService::new(registry, store.clone(), host.clone(), host);
    let compact = service.get_game_info(
        "chess",
        &json!({}),
        None,
        gfa_api_types::InfoDetail::Compact,
    )?;
    let full = service.get_game_info("chess", &json!({}), None, gfa_api_types::InfoDetail::Full)?;
    println!(
        "chess compact={} full={}",
        compact.approx_tokens, full.approx_tokens
    );
    assert!(
        compact.approx_tokens < 1500,
        "compact {}",
        compact.approx_tokens
    );
    assert!(full.approx_tokens < 6000, "full {}", full.approx_tokens);
    let registry = {
        let mut registry = gfa_core::GameRegistry::default();
        registry.register::<gfa_game_chess::ChessGame>()?;
        registry
    };
    let game = registry.get("chess")?;
    let state = game.initial_state(&json!({}), 0)?;
    let native = game.observe(&state, gfa_core::Viewer::Player(0))?;
    let wire: Value = serde_json::from_slice(&serde_json::to_vec(&native)?)?;
    assert_eq!(
        data(&full, "initial_state")?["observation"]["json"],
        wire["json"]
    );
    assert_eq!(
        data(&full, "initial_state")?["observation"]["text"],
        wire["text"]
    );
    assert_eq!(
        data(&full, "initial_state")?["omitted_fields"],
        json!(["action_mask", "observation.tensor"])
    );
    assert_eq!(native.tensor.as_ref().map(|t| t.values.len()), Some(1280));
    assert_eq!(game.action_mask(&state, 0)?.len(), 9345);
    assert!(data(&compact, "action_format")?.get("schema").is_none());
    assert!(data(&full, "action_format")?["schema"].is_object());
    store.close().await;
    Ok(())
}

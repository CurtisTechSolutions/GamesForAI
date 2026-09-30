use super::{
    tests::{call, config, fixture},
    *,
};
use axum::http::StatusCode;
use serde_json::{json, Value};

type TestResult = Result<(), ServerError>;

fn valid(document: &Value, schema: &Value, value: &Value) -> bool {
    let mut root = schema.clone();
    root["components"] = document["components"].clone();
    root["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    jsonschema::is_valid(&root, value)
}

fn response_schema<'a>(doc: &'a Value, path: &str, method: &str, status: &str) -> &'a Value {
    &doc["paths"][path][method]["responses"][status]["content"]["application/json"]["schema"]
}

fn check_refs(root: &Value, value: &Value) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                assert!(
                    reference.starts_with("#/components/schemas/"),
                    "unexpected reference {reference}"
                );
                assert!(
                    root.pointer(&reference[1..]).is_some(),
                    "unresolved reference {reference}"
                );
            }
            for value in object.values() {
                check_refs(root, value);
            }
        }
        Value::Array(values) => {
            for value in values {
                check_refs(root, value);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn served_openapi_matches_real_requests_responses_and_conditional_formats() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    let (status, doc) = call(&app.router, "GET", "/v1/openapi.json", Value::Null, None).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc, serde_json::to_value(gfa_http::openapi_document(true))?);
    check_refs(&doc, &doc);
    for (path, method) in [
        ("/healthz", "get"),
        ("/v1/games", "get"),
        ("/v1/games/{game_id}", "get"),
        ("/v1/games/{game_id}/info", "get"),
        ("/v1/matches", "post"),
        ("/v1/matches", "get"),
        ("/v1/matches/{id}", "get"),
        ("/v1/matches/{id}/resign", "post"),
        ("/v1/matches/{id}/offer-draw", "post"),
        ("/v1/matches/{id}/info", "get"),
        ("/v1/matches/{id}/state", "get"),
        ("/v1/matches/{id}/actions", "post"),
        ("/v1/matches/{id}/replay", "get"),
        ("/v1/matches/{id}/legal-actions", "get"),
        ("/v1/matches/{id}/stream", "get"),
        ("/v1/games/{game_id}/positions/validate", "post"),
        ("/v1/openapi.json", "get"),
    ] {
        assert!(
            doc["paths"][path][method].is_object(),
            "missing {method} {path}"
        );
    }
    assert_eq!(doc["paths"].as_object().map(serde_json::Map::len), Some(16));
    let game_info = &doc["paths"]["/v1/games/{game_id}/info"]["get"];
    assert!(game_info["responses"]["200"]["content"]["text/markdown"].is_object());
    assert!(game_info["responses"]["304"].get("content").is_none());
    for name in ["format", "detail", "config", "seat"] {
        let parameter = game_info["parameters"]
            .as_array()
            .ok_or("parameters")?
            .iter()
            .find(|p| p["name"] == name)
            .ok_or("parameter missing")?;
        assert_ne!(parameter["required"], true, "{name} should be optional");
    }
    for (path, template) in [
        ("/healthz", "/healthz"),
        ("/v1/games", "/v1/games"),
        ("/v1/games/tictactoe", "/v1/games/{game_id}"),
        ("/v1/games/tictactoe/info", "/v1/games/{game_id}/info"),
    ] {
        let (status, response) = call(&app.router, "GET", path, Value::Null, None).await?;
        assert_eq!(status, StatusCode::OK);
        assert!(
            valid(
                &doc,
                response_schema(&doc, template, "get", "200"),
                &response
            ),
            "invalid response for {path}"
        );
    }
    let create = &doc["paths"]["/v1/matches"]["post"]["requestBody"]["content"]["application/json"]
        ["schema"];
    assert!(valid(&doc, create, &json!({"game_id":"tictactoe"})));
    assert!(!valid(
        &doc,
        create,
        &json!({"game_id":"tictactoe","typo":true})
    ));
    let (_, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({"game_id":"tictactoe","seed":42}),
        None,
    )
    .await?;
    assert!(valid(
        &doc,
        response_schema(&doc, "/v1/matches", "post", "201"),
        &initial
    ));
    let id = initial["match_id"].as_str().ok_or("id")?;
    for (path, template) in [
        ("/v1/matches".to_owned(), "/v1/matches"),
        (format!("/v1/matches/{id}"), "/v1/matches/{id}"),
    ] {
        let (_, value) = call(&app.router, "GET", &path, Value::Null, None).await?;
        assert!(valid(
            &doc,
            response_schema(&doc, template, "get", "200"),
            &value
        ));
    }
    let (_, moved) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/actions"),
        json!({"seat":0,"turn":0,"action":"r2c2"}),
        Some("openapi"),
    )
    .await?;
    assert!(valid(
        &doc,
        response_schema(&doc, "/v1/matches/{id}/actions", "post", "200"),
        &moved
    ));
    for suffix in ["state", "info", "legal-actions", "replay"] {
        let (_, response) = call(
            &app.router,
            "GET",
            &format!("/v1/matches/{id}/{suffix}?seat=0"),
            Value::Null,
            None,
        )
        .await?;
        assert!(
            valid(
                &doc,
                response_schema(&doc, &format!("/v1/matches/{{id}}/{suffix}"), "get", "200"),
                &response
            ),
            "invalid {suffix}"
        );
    }
    let moves = &doc["paths"]["/v1/matches/{id}/actions"]["post"]["requestBody"]["content"]
        ["application/json"]["schema"];
    for action in [json!("r1c1"), json!({"row":1,"col":1}), json!({"index":0})] {
        assert!(valid(
            &doc,
            moves,
            &json!({"seat":0,"turn":0,"action":action})
        ));
    }
    for request in [
        json!({"seat":0,"action":"r1c1"}),
        json!({"seat":0,"turn":-1,"action":"r1c1"}),
        json!({"seat":256,"turn":0,"action":"r1c1"}),
    ] {
        assert!(!valid(&doc, moves, &request), "accepted invalid {request}");
    }
    let (_, error) = call(
        &app.router,
        "GET",
        "/v1/matches/missing/state",
        Value::Null,
        None,
    )
    .await?;
    assert!(valid(
        &doc,
        response_schema(&doc, "/v1/matches/{id}/state", "get", "default"),
        &error
    ));
    let (_, position) = call(&app.router,"POST","/v1/games/tictactoe/positions/validate",
        json!({"start":{"state":{"board":[null,null,null,null,null,null,null,null,null],"to_move":0}},"seed":42}),None).await?;
    assert!(valid(
        &doc,
        response_schema(
            &doc,
            "/v1/games/{game_id}/positions/validate",
            "post",
            "200"
        ),
        &position
    ));
    let state_schema = json!({"$ref":"#/components/schemas/StreamMessage"});
    assert!(valid(
        &doc,
        &state_schema,
        &json!({"type":"state","state":moved["state"]})
    ));
    assert!(valid(
        &doc,
        &state_schema,
        &json!({"type":"error","error":error["error"]})
    ));
    app.store.close().await;
    Ok(())
}

#[test]
fn rest_only_contract_excludes_uninstalled_streaming() -> TestResult {
    let doc = serde_json::to_value(gfa_http::openapi_document(false))?;
    assert!(doc["paths"].get("/v1/matches/{id}/stream").is_none());
    check_refs(&doc, &doc);
    Ok(())
}

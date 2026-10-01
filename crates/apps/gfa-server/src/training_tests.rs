use super::{
    tests::{call, config, fixture},
    *,
};
use axum::http::StatusCode;
use serde_json::{json, Value};

type TestResult = Result<(), ServerError>;

async fn batch(app: &Application, operations: Value) -> Result<Value, ServerError> {
    let (status, body) = call(
        &app.router, "POST", "/v1/batch/step", json!({"operations": operations}), None,
    ).await?;
    assert_eq!(status, StatusCode::OK, "{body}");
    Ok(body)
}

#[tokio::test]
async fn training_batches_create_step_reset_branch_and_never_persist() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    let initial = batch(&app, json!([
        {"op":"create","game_id":"tictactoe","seed":42,"seat":0},
        {"op":"create","game_id":"connect4","seed":43,"seat":1},
        {"op":"create","game_id":"missing","seed":0,"seat":0}
    ])).await?;
    assert_eq!(initial["results"][0]["status"], "ok");
    assert_eq!(initial["results"][1]["frame"]["action_mask"], json!(vec![false; 7]));
    assert_eq!(initial["results"][2]["error"]["code"], "UNKNOWN_GAME");
    let checkpoint = initial["results"][0]["checkpoint"].clone();
    let request = json!([
        {"op":"step","checkpoint":checkpoint,"seat":0,"action":4},
        {"op":"step","checkpoint":checkpoint,"seat":0,"action":0},
        {"op":"step","checkpoint":checkpoint,"seat":1,"action":4}
    ]);
    let first = batch(&app, request.clone()).await?;
    assert_eq!(first, batch(&app, request).await?);
    assert_eq!(first["results"][0]["frame"]["to_act"], json!([1]));
    assert_eq!(first["results"][0]["frame"]["rewards"], json!([0.0, 0.0]));
    assert_ne!(first["results"][0]["checkpoint"], first["results"][1]["checkpoint"]);
    assert_eq!(first["results"][2]["status"], "error");
    let moved = &first["results"][0]["checkpoint"];
    let frames = batch(&app, json!([
        {"op":"observe","checkpoint":moved,"seat":1},
        {"op":"observe","checkpoint":moved,"seat":9},
        {"op":"reset","checkpoint":moved,"seed":42,"seat":0}
    ])).await?;
    assert_eq!(frames["results"][0]["frame"]["legal_actions"].as_array().map(Vec::len), Some(8));
    assert_eq!(frames["results"][1]["status"], "error");
    assert_eq!(frames["results"][2]["checkpoint"], checkpoint);
    assert!(frames["results"][0]["frame"].get("checkpoint").is_none());
    let (_, history) = call(&app.router, "GET", "/v1/matches", Value::Null, None).await?;
    assert_eq!(history["matches"], json!([]));
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn batch_rewards_endings_and_invalid_checkpoints_match_native_rules() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    let created = batch(&app, json!([
        {"op":"create","game_id":"tictactoe","seed":0,"seat":0},
        {"op":"create","game_id":"chess","config":{"max_plies":1},"seed":0,"seat":0}
    ])).await?;
    let mut checkpoint = created["results"][0]["checkpoint"].clone();
    for (turn, action) in [0, 3, 1, 4, 2].into_iter().enumerate() {
        let result = batch(&app, json!([
            {"op":"step","checkpoint":checkpoint,"seat":turn%2,"action":action}
        ])).await?;
        assert_eq!(result["results"][0]["status"], "ok");
        if turn == 4 {
            assert_eq!(result["results"][0]["frame"]["terminated"], true);
            assert_eq!(result["results"][0]["frame"]["rewards"], json!([1.0,-1.0]));
        }
        checkpoint = result["results"][0]["checkpoint"].clone();
    }
    let ended = batch(&app, json!([
        {"op":"step","checkpoint":checkpoint,"seat":1,"action":8}
    ])).await?;
    assert_eq!(ended["results"][0]["error"]["code"], "MATCH_FINISHED");
    let chess = &created["results"][1];
    let capped = batch(&app, json!([{
        "op":"step","checkpoint":chess["checkpoint"],"seat":0,
        "action":chess["frame"]["legal_actions"][0]["index"]
    }])).await?;
    assert_eq!(capped["results"][0]["frame"]["truncated"], true);
    assert_eq!(capped["results"][0]["frame"]["terminated"], false);
    checkpoint["engine_version"] = json!("incompatible");
    let bad = batch(&app, json!([
        {"op":"observe","checkpoint":checkpoint,"seat":0}
    ])).await?;
    assert_eq!(bad["results"][0]["error"]["code"], "INVALID_POSITION");
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn batch_limits_and_route_body_override_are_enforced() -> TestResult {
    let dir = tempfile::tempdir()?;
    let app = fixture(&config(&dir)).await?;
    let create = json!({"op":"create","game_id":"tictactoe","seed":0,"seat":0});
    for operations in [vec![], vec![create.clone(); 65]] {
        let (status, body) = call(&app.router, "POST", "/v1/batch/step",
            json!({"operations":operations}), None).await?;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "BATCH_LIMIT");
    }
    // Training accepts more than the standard route's 64 KiB.
    let result = batch(&app, json!([{
        "op":"create","game_id":"tictactoe","seed":0,"seat":0,
        "config":{"unknown": "x".repeat(70000)}
    }])).await?;
    assert_eq!(result["results"][0]["error"]["code"], "INVALID_CONFIG");
    let (status, _) = call(&app.router, "POST", "/v1/batch/step", json!({
        "operations":[{"op":"create","game_id":"tictactoe","seed":0,"seat":0,
                       "config":{"unknown":"x".repeat(4 * 1024 * 1024)}}]
    }), None).await?;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, _) = call(&app.router, "POST", "/v1/batch/step", json!({
        "operations":[{"op":"create","game_id":"tictactoe","seed":0,"seat":0,"match_id":"private"}]
    }), None).await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Direct trusted service calls also bound output even without HTTP input limits.
    let request = serde_json::from_value(json!({"operations":vec![json!({
        "op":"create","game_id":"x".repeat(200000),"seed":0,"seat":0
    });64]}))?;
    let result = app.service.training_batch(request)?;
    assert_eq!(result.results.len(), 64);
    assert!(serde_json::to_vec(&result)?.len() <= 8 * 1024 * 1024);
    assert!(serde_json::to_value(result)?["results"].as_array().ok_or("results")?
        .iter().any(|row| row["error"]["code"] == "BATCH_LIMIT"));
    app.store.close().await;
    Ok(())
}

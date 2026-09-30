use super::{tests::{call, config, fixture}, *};
use axum::http::StatusCode;
use serde_json::{json, Value};

#[tokio::test]
async fn event_http_pages_round_trip_across_restart_and_redact_internal_fork_metadata() -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let options = config(&directory);
    let app = fixture(&options).await?;
    let (_, created) = call(&app.router,"POST","/v1/matches",json!({"game_id":"tictactoe","seed":42}),None).await?;
    let id = created["match_id"].as_str().ok_or("id")?;
    call(&app.router,"POST",&format!("/v1/matches/{id}/actions"),json!({"seat":0,"turn":0,"action":"r2c2","reasoning":"Take the center."}),None).await?;
    let (status, first) = call(&app.router,"GET",&format!("/v1/matches/{id}/events?limit=1"),Value::Null,None).await?;
    assert_eq!(status,StatusCode::OK);
    assert_eq!(first["next"],0);
    assert!(first["events"][0].get("seed").is_none());
    let (_, second) = call(&app.router,"GET",&format!("/v1/matches/{id}/events?since=0&seat=0"),Value::Null,None).await?;
    assert_eq!(second["events"][0]["sequence"],1);
    assert_eq!(second["events"][0]["reasoning"],"Take the center.");
    let (_, spectator) = call(&app.router,"GET",&format!("/v1/matches/{id}/events?since=0"),Value::Null,None).await?;
    assert_eq!(spectator["events"][0]["action"]["string"],"r2c2");
    assert!(spectator["events"][0].get("reasoning").is_none());
    let (_, child) = call(&app.router,"POST",&format!("/v1/matches/{id}/fork"),json!({"turn":1}),None).await?;
    let child_id = child["match_id"].as_str().ok_or("child")?;
    let (_, branch) = call(&app.router,"GET",&format!("/v1/matches/{child_id}/events"),Value::Null,None).await?;
    assert_eq!(branch["events"][0]["type"],"forked_from");
    assert_eq!(branch["events"][0]["source"]["match_id"],id);
    assert!(branch["events"][0].get("parent_revision").is_none());
    assert!(branch["events"][0].get("viewer").is_none());
    assert_eq!(branch["events"][1]["type"],"created");
    for suffix in ["limit=0","limit=101","since=99"] {
        assert_eq!(call(&app.router,"GET",&format!("/v1/matches/{id}/events?{suffix}"),Value::Null,None).await?.0,StatusCode::UNPROCESSABLE_ENTITY);
    }
    app.store.close().await;
    let reopened = fixture(&options).await?;
    assert_eq!(call(&reopened.router,"GET",&format!("/v1/matches/{id}/events?since=0&seat=0"),Value::Null,None).await?.1,second);
    reopened.store.close().await;
    Ok(())
}

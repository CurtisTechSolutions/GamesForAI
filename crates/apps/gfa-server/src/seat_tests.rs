use super::{
    tests::{call, config, fixture},
    *,
};
use axum::http::StatusCode;
use serde_json::{json, Value};

fn bot() -> Value {
    json!({"type":"opponent","opponent":{"id":"minimax","level":3},"seed":19})
}

#[tokio::test]
async fn human_match_returns_bot_replies_deduplicates_concurrent_requests_and_restarts(
) -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let options = config(&directory);
    let app = fixture(&options).await?;
    let (status, initial) = call(
        &app.router,
        "POST",
        "/v1/matches",
        json!({
            "game_id":"tictactoe","seed":42,"seats":[{"type":"self"},bot()]
        }),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{initial}");
    let id = initial["match_id"].as_str().ok_or("id")?;
    let path = format!("/v1/matches/{id}/actions");
    let request = json!({"seat":0,"turn":0,"action":"r2c2"});
    let (left, right) = tokio::join!(
        call(
            &app.router,
            "POST",
            &path,
            request.clone(),
            Some("same-request")
        ),
        call(
            &app.router,
            "POST",
            &path,
            request.clone(),
            Some("same-request")
        ),
    );
    let (status, first) = left?;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(right?.1, first);
    assert_eq!(first["state"]["turn"], 2);
    assert_eq!(first["opponent_replies"][0]["seat"], 1);
    assert!(first["opponent_replies"][0]["action"]["string"].is_string());
    let mut state = first["state"].clone();
    for _ in 0..5 {
        if state["terminated"] == true {
            break;
        }
        let action = state["legal_actions"][0]["string"].clone();
        let (status, moved) = call(
            &app.router,
            "POST",
            &path,
            json!({"seat":0,"turn":state["turn"],"action":action}),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{moved}");
        state = moved["state"].clone();
    }
    assert_eq!(state["terminated"], true);
    assert_eq!(
        call(
            &app.router,
            "POST",
            &path,
            request.clone(),
            Some("same-request")
        )
        .await?
        .1,
        first
    );
    let (_, replay) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{id}/replay?seat=0"),
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(
        replay["states"].as_array().ok_or("states")?.last(),
        Some(&state)
    );
    app.store.close().await;
    let reopened = fixture(&options).await?;
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
    assert_eq!(
        call(
            &reopened.router,
            "POST",
            &path,
            request,
            Some("same-request")
        )
        .await?
        .1,
        first
    );
    reopened.store.close().await;
    Ok(())
}

#[tokio::test]
async fn opponent_openings_and_fork_seat_replacement_preserve_the_parent() -> Result<(), ServerError>
{
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let (status, initial) = call(
        &app.router,
        "POST",
        "/v1/matches?seat=1",
        json!({
            "game_id":"connect4","seed":42,"seats":[bot(),{"type":"human"}]
        }),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{initial}");
    assert_eq!(initial["turn"], 1);
    assert_eq!(initial["to_act"], json!([1]));
    let id = initial["match_id"].as_str().ok_or("id")?;
    let (_, before) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{id}/replay?seat=1"),
        Value::Null,
        None,
    )
    .await?;
    let (status, child) = call(
        &app.router,
        "POST",
        &format!("/v1/matches/{id}/fork?seat=1"),
        json!({
            "turn":1,"seats":[{"type":"self"},{"type":"human"}]
        }),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED, "{child}");
    assert_eq!(child["turn"], 0);
    assert_eq!(child["to_act"], json!([1]));
    let child_id = child["match_id"].as_str().ok_or("child")?;
    let (_, metadata) = call(
        &app.router,
        "GET",
        &format!("/v1/matches/{child_id}"),
        Value::Null,
        None,
    )
    .await?;
    assert_eq!(metadata["seats"], json!([{"type":"self"},{"type":"human"}]));
    assert_eq!(
        call(
            &app.router,
            "GET",
            &format!("/v1/matches/{id}/replay?seat=1"),
            Value::Null,
            None
        )
        .await?
        .1,
        before
    );
    app.store.close().await;
    Ok(())
}

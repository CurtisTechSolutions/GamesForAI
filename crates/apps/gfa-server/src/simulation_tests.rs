use super::{tests::{call, config, fixture}, *};
use axum::http::StatusCode;
use serde_json::{json, Value};

type TestResult = Result<(), ServerError>;

#[tokio::test]
async fn simulation_matches_live_play_and_records_usage_without_changing_replay() -> TestResult {
    let directory = tempfile::tempdir()?;
    let settings = config(&directory);
    let app = fixture(&settings).await?;
    for (game, line) in [
        ("tictactoe", vec![json!("r1c1"), json!("r2c1"), json!("r1c2"), json!("r2c2"), json!("r1c3")]),
        ("connect4", vec![json!("1"), json!("2"), json!("1"), json!("2"), json!("1"), json!("2"), json!("1")]),
    ] {
        let (_, initial) = call(&app.router, "POST", "/v1/matches", json!({"game_id":game,"seed":42}), None).await?;
        let id = initial["match_id"].as_str().ok_or("id")?;
        let replay_url = format!("/v1/matches/{id}/replay?seat=0");
        let (_, before) = call(&app.router, "GET", &replay_url, Value::Null, None).await?;
        let (status, simulated) = call(&app.router, "POST", &format!("/v1/games/{game}/simulate"),
            json!({"from":{"match_id":id,"seat":0},"lines":[line,line],"seed":42,"return":"all"}), None).await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(simulated["lines"][0], simulated["lines"][1]);
        assert_eq!(simulated["lines"][0]["moves_applied"], line.len());
        let (_, after) = call(&app.router, "GET", &replay_url, Value::Null, None).await?;
        assert_eq!(before, after);
        for (turn, action) in line.iter().enumerate() {
            call(&app.router, "POST", &format!("/v1/matches/{id}/actions"),
                json!({"turn":turn,"seat":turn%2,"action":action}), None).await?;
            let (_, mut state) = call(&app.router, "GET", &format!("/v1/matches/{id}/state?seat=0"), Value::Null, None).await?;
            state["match_id"] = json!("");
            assert_eq!(state, simulated["lines"][0]["states"][turn]);
        }
        let (_, metadata) = call(&app.router, "GET", &format!("/v1/matches/{id}"), Value::Null, None).await?;
        assert_eq!(metadata["assist_usage"], json!([{"seat":0,"simulation_calls":1,"simulated_moves":line.len()*2}]));
    }
    app.store.close().await;
    let app = fixture(&settings).await?;
    let (_, history) = call(&app.router, "GET", "/v1/matches", Value::Null, None).await?;
    for item in history["matches"].as_array().ok_or("matches")? {
        assert_eq!(item["assist_usage"][0]["simulation_calls"], 1);
    }
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn simulation_policy_blocks_match_and_standalone_bypasses() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let (_, initial) = call(&app.router, "POST", "/v1/matches",
        json!({"game_id":"tictactoe","assists":{"allow_simulation":false}}), None).await?;
    let id = initial["match_id"].as_str().ok_or("id")?;
    let position = initial["observation"]["json"].clone();
    for source in [
        json!({"match_id":id,"seat":0}),
        json!({"state":position}),
        json!({"position":position.to_string()}),
    ] {
        let (status, error) = call(&app.router, "POST", "/v1/games/tictactoe/simulate",
            json!({"from":source,"lines":[["r2c2"]],"seed":99}), None).await?;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error["error"]["code"], "ASSIST_NOT_ALLOWED");
    }
    let (_, info) = call(&app.router, "GET", &format!("/v1/matches/{id}/info?seat=0"), Value::Null, None).await?;
    assert_eq!(info["sections"][0]["data"]["assists"]["allow_simulation"], false);
    let (_, metadata) = call(&app.router, "GET", &format!("/v1/matches/{id}"), Value::Null, None).await?;
    assert_eq!(metadata["assist_usage"], json!([]));
    call(&app.router, "POST", &format!("/v1/matches/{id}/resign"), json!({"seat":0,"turn":0}), None).await?;
    let (status, simulated) = call(&app.router, "POST", "/v1/games/tictactoe/simulate",
        json!({"from":{"state":position},"lines":[["r2c2"]],"seed":99}), None).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(simulated["lines"][0]["moves_applied"], 1);
    app.store.close().await;
    Ok(())
}

#[tokio::test]
async fn simulation_rejects_bad_sources_and_isolates_illegal_lines() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = fixture(&config(&directory)).await?;
    let (_, initial) = call(&app.router, "POST", "/v1/matches", json!({"game_id":"tictactoe"}), None).await?;
    let id = initial["match_id"].as_str().ok_or("id")?;
    let source = json!({"match_id":id,"seat":0});
    let (_, result) = call(&app.router, "POST", "/v1/games/tictactoe/simulate",
        json!({"from":source,"lines":[["r1c1","r1c1"],["r3c3"],[]],"seed":7}), None).await?;
    assert_eq!(result["lines"][0]["error"]["index"], 1);
    assert_eq!(result["lines"][0]["moves_applied"], 1);
    assert_eq!(result["lines"][1]["moves_applied"], 1);
    assert_eq!(result["lines"][2]["states"][0]["turn"], 0);
    for body in [
        json!({"from":source,"lines":vec![json!([]);17]}),
        json!({"from":source,"lines":[vec![json!("r1c1");33]]}),
        json!({"from":{"match_id":id,"seat":0,"turn":999},"lines":[[]]}),
        json!({"from":source,"lines":[[]],"config":{"unsupported":true}}),
        json!({"from":{"state":{},"position":"bad"},"lines":[[]]}),
    ] {
        let (status, error) = call(&app.router, "POST", "/v1/games/tictactoe/simulate", body, None).await?;
        assert!(status.is_client_error());
        assert!(error["error"]["code"].is_string());
    }
    let (status, _) = call(&app.router, "POST", "/v1/games/tictactoe/simulate?seat=1",
        json!({"from":source,"lines":[[]]}), None).await?;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app.router, "POST", "/v1/games/connect4/simulate",
        json!({"from":source,"lines":[[]]}), None).await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    app.store.close().await;
    Ok(())
}

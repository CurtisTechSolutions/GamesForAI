use super::*;
use futures_util::{SinkExt, StreamExt};
use gfa_api_types::{CreateMatch, MoveRequest};
use gfa_core::Viewer;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type TestResult = Result<(), ServerError>;

async fn next_state(client: &mut Client) -> Result<Value, ServerError> {
    let message = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await?
        .ok_or("stream ended before state")??;
    let value: Value = serde_json::from_str(message.to_text()?)?;
    assert_eq!(value["type"], "state");
    Ok(value["state"].clone())
}

async fn create(service: &GameService) -> Result<String, ServerError> {
    let state = service
        .create_match(
            CreateMatch {
                game_id: "tictactoe".into(),
                config: json!({}),
                seed: Some(42),
                start: None,
                include_info: true,
                seats: vec![],
                assists: gfa_api_types::Assists::default(),
            },
            Viewer::Player(0),
        )
        .await?;
    Ok(state.match_id)
}

#[tokio::test]
async fn streams_preserve_turn_order_and_reconnect_from_current_state() -> TestResult {
    let dir = tempfile::tempdir()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let app = application(
        &Config {
            database: Database::Sqlite(dir.path().join("live.sqlite")),
            port: 0,
        },
        address,
    )
    .await?;
    let service = app.service.clone();
    let id = create(&service).await?;
    let other = create(&service).await?;
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(serve_application(listener, app, async {
        let _ = stopped.await;
    }));
    let (mut player, _) =
        connect_async(format!("ws://{address}/v1/matches/{id}/stream?seat=0")).await?;
    let (mut spectator, _) =
        connect_async(format!("ws://{address}/v1/matches/{id}/stream")).await?;
    assert_eq!(next_state(&mut player).await?["turn"], 0);
    assert_eq!(
        next_state(&mut spectator).await?["legal_actions"],
        json!([])
    );
    // A notification for another match must not enter this stream.
    service
        .make_move(
            &other,
            MoveRequest {
                seat: 0,
                turn: 0,
                action: json!("r3c3"),
                reasoning: None,
            },
            None,
        )
        .await?;
    for (turn, action) in ["r1c1", "r2c1", "r1c2"].into_iter().enumerate() {
        service
            .make_move(
                &id,
                MoveRequest {
                    seat: (turn % 2) as u8,
                    turn: turn as u64,
                    action: json!(action),
                    reasoning: None,
                },
                None,
            )
            .await?;
    }
    for turn in 1..=3 {
        assert_eq!(next_state(&mut player).await?["turn"], turn);
        let public = next_state(&mut spectator).await?;
        assert_eq!(public["turn"], turn);
        assert_eq!(public["legal_actions"], json!([]));
        assert_eq!(public["action_mask"], json!([]));
    }
    let (mut reconnected, _) =
        connect_async(format!("ws://{address}/v1/matches/{id}/stream?seat=1")).await?;
    assert_eq!(next_state(&mut reconnected).await?["turn"], 3);
    reconnected.send(Message::Text("move".into())).await?;
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), reconnected.next()).await?,
        Some(Ok(Message::Close(_)))
    ));
    stop.send(()).map_err(|_| "shutdown receiver closed")?;
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), player.next()).await?,
        Some(Ok(Message::Close(_)))
    ));
    tokio::time::timeout(Duration::from_secs(5), task).await???;
    Ok(())
}

#[tokio::test]
async fn invalid_view_missing_match_and_foreign_origin_fail_before_upgrade() -> TestResult {
    let dir = tempfile::tempdir()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let app = application(
        &Config {
            database: Database::Sqlite(dir.path().join("live.sqlite")),
            port: 0,
        },
        address,
    )
    .await?;
    let id = create(&app.service).await?;
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(serve_application(listener, app, async {
        let _ = stopped.await;
    }));
    for (path, expected) in [
        ("/v1/matches/missing/stream".to_string(), 404),
        (format!("/v1/matches/{id}/stream?seat=255"), 422),
    ] {
        match connect_async(format!("ws://{address}{path}")).await {
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                assert_eq!(response.status().as_u16(), expected)
            }
            _ => return Err("invalid stream unexpectedly upgraded".into()),
        }
    }
    let mut request = format!("ws://{address}/v1/matches/{id}/stream").into_client_request()?;
    request
        .headers_mut()
        .insert("origin", "http://foreign.example".parse()?);
    match connect_async(request).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            assert_eq!(response.status().as_u16(), 403)
        }
        _ => return Err("foreign origin unexpectedly upgraded".into()),
    }
    stop.send(()).map_err(|_| "shutdown receiver closed")?;
    tokio::time::timeout(Duration::from_secs(5), task).await???;
    Ok(())
}

#[tokio::test]
async fn periodic_replay_recovers_commits_without_an_in_process_notification() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("live.sqlite");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let app = application(
        &Config {
            database: Database::Sqlite(path.clone()),
            port: 0,
        },
        address,
    )
    .await?;
    let id = create(&app.service).await?;
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(serve_application(listener, app, async {
        let _ = stopped.await;
    }));
    let (mut client, _) = connect_async(format!("ws://{address}/v1/matches/{id}/stream")).await?;
    assert_eq!(next_state(&mut client).await?["turn"], 0);
    let store = Arc::new(SqliteMatchStore::open(&path).await?);
    let separate = GameService::new(
        gfa_games::registry()?,
        store.clone(),
        Arc::new(Host),
        Arc::new(Host),
    );
    separate
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
    assert_eq!(next_state(&mut client).await?["turn"], 1);
    stop.send(()).map_err(|_| "shutdown receiver closed")?;
    tokio::time::timeout(Duration::from_secs(5), task).await???;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn terminal_state_is_delivered_before_the_stream_closes() -> TestResult {
    let dir = tempfile::tempdir()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let app = application(
        &Config {
            database: Database::Sqlite(dir.path().join("live.sqlite")),
            port: 0,
        },
        address,
    )
    .await?;
    let service = app.service.clone();
    let id = create(&service).await?;
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(serve_application(listener, app, async {
        let _ = stopped.await;
    }));
    let url = format!("ws://{address}/v1/matches/{id}/stream");
    let (mut client, _) = connect_async(&url).await?;
    assert_eq!(next_state(&mut client).await?["turn"], 0);
    for (turn, action) in ["r1c1", "r2c1", "r1c2", "r2c2", "r1c3"]
        .into_iter()
        .enumerate()
    {
        service
            .make_move(
                &id,
                MoveRequest {
                    seat: (turn % 2) as u8,
                    turn: turn as u64,
                    action: json!(action),
                    reasoning: None,
                },
                None,
            )
            .await?;
    }
    for turn in 1..=5 {
        let state = next_state(&mut client).await?;
        assert_eq!(state["turn"], turn);
        if turn == 5 {
            assert_eq!(state["terminated"], true);
        }
    }
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), client.next()).await?,
        Some(Ok(Message::Close(_)))
    ));
    let (mut terminal, _) = connect_async(&url).await?;
    assert_eq!(next_state(&mut terminal).await?["terminated"], true);
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), terminal.next()).await?,
        Some(Ok(Message::Close(_)))
    ));
    stop.send(()).map_err(|_| "shutdown receiver closed")?;
    tokio::time::timeout(Duration::from_secs(5), task).await???;
    Ok(())
}

#[tokio::test]
async fn control_updates_are_delivered_even_when_turn_does_not_change() -> TestResult {
    use gfa_api_types::ControlRequest;
    let directory = tempfile::tempdir()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let app = application(
        &Config {
            database: Database::Sqlite(directory.path().join("controls.sqlite")),
            port: 0,
        },
        address,
    )
    .await?;
    let service = app.service.clone();
    let id = create(&service).await?;
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(serve_application(listener, app, async {
        let _ = stopped.await;
    }));
    let (mut client, _) = connect_async(format!("ws://{address}/v1/matches/{id}/stream")).await?;
    assert_eq!(next_state(&mut client).await?["turn"], 0);
    service
        .offer_draw(&id, ControlRequest { seat: 0, turn: 0 })
        .await?;
    let offered = next_state(&mut client).await?;
    assert_eq!(offered["turn"], 0);
    assert_eq!(offered["draw_offer"], 0);
    service
        .offer_draw(&id, ControlRequest { seat: 1, turn: 0 })
        .await?;
    let ended = next_state(&mut client).await?;
    assert_eq!(ended["turn"], 0);
    assert_eq!(ended["terminated"], true);
    assert_eq!(ended["outcome"]["reason"], "agreed_draw");
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), client.next()).await?,
        Some(Ok(Message::Close(_)))
    ));
    stop.send(()).map_err(|_| "shutdown receiver closed")?;
    tokio::time::timeout(Duration::from_secs(5), task).await???;
    Ok(())
}

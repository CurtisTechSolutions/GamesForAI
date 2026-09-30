use super::{
    tests::{config, fixture},
    *,
};
use futures_util::StreamExt;
use gfa_api_types::{CreateMatch, Seat};
use gfa_core::Viewer;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::oneshot;

#[tokio::test]
async fn server_resumes_an_unfinished_bot_match_streams_it_and_persists_completion(
) -> Result<(), ServerError> {
    let directory = tempfile::tempdir()?;
    let options = config(&directory);
    let app = fixture(&options).await?;
    let bot: Seat =
        serde_json::from_value(json!({"type":"opponent","opponent":{"id":"random"},"seed":71}))?;
    let request: CreateMatch =
        serde_json::from_value(json!({"game_id":"tictactoe","seed":42,"seats":[bot.clone(),bot]}))?;
    let initial = app.service.create_match(request, Viewer::Player(0)).await?;
    assert_eq!(initial.turn, 0);
    assert_eq!(
        app.service.advance_opponents(&initial.match_id, 2).await?,
        2
    );
    app.store.close().await;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let restarted = application(&options, address).await?;
    let service = restarted.service.clone();
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(serve_application(listener, restarted, async {
        let _ = stopped.await;
    }));
    let (mut stream, _) = tokio_tungstenite::connect_async(format!(
        "ws://{address}/v1/matches/{}/stream",
        initial.match_id
    ))
    .await?;
    let final_state = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = stream.next().await.ok_or("stream ended")??;
            let value: Value = serde_json::from_str(message.to_text()?)?;
            if value["state"]["terminated"] == true {
                return Ok::<Value, ServerError>(value["state"].clone());
            }
        }
    })
    .await??;
    let replay = service
        .get_replay(&initial.match_id, Viewer::Spectator)
        .await?;
    assert_eq!(serde_json::to_value(replay.states.last())?, final_state);
    stop.send(()).map_err(|_| "shutdown")?;
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    let reopened = fixture(&options).await?;
    assert_eq!(
        serde_json::to_value(
            reopened
                .service
                .get_replay(&initial.match_id, Viewer::Spectator)
                .await?
        )?,
        serde_json::to_value(replay)?
    );
    reopened.store.close().await;
    Ok(())
}

#[tokio::test]
async fn busy_workers_back_off_instead_of_spinning_on_the_same_match() -> Result<(), ServerError> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Busy(Arc<AtomicUsize>);
    impl gfa_service::OpponentExecutor for Busy {
        fn execute(&self, _: gfa_service::OpponentJob) -> gfa_service::OpponentFuture<'_> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(gfa_api_types::ApiError::new("ENGINE_BUSY", "busy", "retry")) })
        }
    }
    let store = Arc::new(SqliteMatchStore::in_memory().await?);
    let host = Arc::new(Host);
    let attempts = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        GameService::new(gfa_games::registry()?, store.clone(), host.clone(), host).with_opponents(
            Arc::new(gfa_service::BuiltinOpponentFactory),
            Arc::new(Busy(attempts.clone())),
        ),
    );
    let bot = json!({"type":"opponent","opponent":{"id":"random"},"seed":71});
    service
        .create_match(
            serde_json::from_value(json!({"game_id":"tictactoe","seats":[bot.clone(),bot]}))?,
            Viewer::Player(0),
        )
        .await?;
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(super::runner::run(service, stopped));
    tokio::time::sleep(Duration::from_millis(450)).await;
    stop.send(true)?;
    task.await?;
    assert!((1..=2).contains(&attempts.load(Ordering::SeqCst)));
    store.close().await;
    Ok(())
}

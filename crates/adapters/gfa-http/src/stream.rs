use crate::{error::HttpError, id, view, Id, View};
use axum::{
    extract::{
        ws::{
            rejection::WebSocketUpgradeRejection, CloseFrame, Message, WebSocket, WebSocketUpgrade,
        },
        State,
    },
    http::StatusCode,
    response::Response,
    Extension,
};
use gfa_api_types::{MatchState, StreamMessage};
use gfa_core::Viewer;
use gfa_service::{GameService, MatchObserver};
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, watch, OwnedSemaphorePermit, Semaphore};

const MAX_STREAMS: u32 = 64;

/// Bounded, best-effort commit signals for local WebSocket clients.
///
/// Only match IDs cross this channel. Each client obtains viewer-scoped states
/// from the service, with durable replay filling notification gaps.
pub struct LiveUpdates {
    changed: broadcast::Sender<String>,
    shutdown: watch::Sender<bool>,
    sessions: Arc<Semaphore>,
}

impl Default for LiveUpdates {
    fn default() -> Self {
        Self {
            changed: broadcast::channel(64).0,
            shutdown: watch::channel(false).0,
            sessions: Arc::new(Semaphore::new(MAX_STREAMS as usize)),
        }
    }
}

impl MatchObserver for LiveUpdates {
    fn committed(&self, id: &str) {
        let _ = self.changed.send(id.into());
    }
}

impl LiveUpdates {
    /// Stop accepting new streams and signal existing streams to close.
    pub fn close(&self) {
        self.shutdown.send_replace(true);
    }

    /// Wait for stream tasks to release their resources before closing storage.
    pub async fn wait_closed(&self) {
        self.close();
        let _ = self.sessions.acquire_many(MAX_STREAMS).await;
    }
}

struct Subscription {
    service: Arc<GameService>,
    id: String,
    viewer: Viewer,
    changes: broadcast::Receiver<String>,
    shutdown: watch::Receiver<bool>,
    _permit: OwnedSemaphorePermit,
}

#[utoipa::path(
    get, path = "/v1/matches/{id}/stream", tag = "Streaming",

    params(("id" = String, Path, description = "Match identifier"),("seat" = Option<u8>, Query, description = "Zero-based seat; omit for spectator view. Creation and position validation default to seat 0.", minimum = 0, maximum = 255)),
    responses((status = 101, description = "WebSocket state frames: current state then each committed turn; terminal frame before close. Submit moves through REST. Reconnect begins at the current state."), (status = 429, description = "At most 64 streams", body = gfa_api_types::ErrorResponse), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
pub(crate) async fn upgrade(
    State(service): State<Arc<GameService>>,
    Extension(updates): Extension<Arc<LiveUpdates>>,
    path: Id,
    query: View,
    websocket: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, HttpError> {
    if *updates.shutdown.borrow() {
        return Err(HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "SHUTTING_DOWN",
            "Server is shutting down",
            "Reconnect after the server restarts.",
        ));
    }
    let id = id(path)?;
    let viewer = view(query)?.viewer();
    let permit = updates.sessions.clone().try_acquire_owned().map_err(|_| {
        HttpError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            "Too many live streams",
            "Close an existing stream before reconnecting.",
        )
    })?;
    // Subscribe before reading so a commit during the initial read cannot be lost.
    let changes = updates.changed.subscribe();
    let shutdown = updates.shutdown.subscribe();
    let initial = service.get_state(&id, viewer).await?;
    let websocket = websocket.map_err(|error| {
        HttpError::new(
            error.status(),
            "INVALID_REQUEST",
            error.body_text(),
            "Connect with a WebSocket client.",
        )
    })?;
    let subscription = Subscription {
        service,
        id,
        viewer,
        changes,
        shutdown,
        _permit: permit,
    };
    Ok(websocket
        .max_message_size(4096)
        .max_frame_size(4096)
        .max_write_buffer_size(1024 * 1024)
        .on_upgrade(move |socket| subscription.run(socket, initial)))
}

async fn send(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(3), socket.send(message)).await,
        Ok(Ok(()))
    )
}

async fn send_state(socket: &mut WebSocket, state: &MatchState) -> bool {
    match serde_json::to_string(&StreamMessage::State {
        state: state.clone(),
    }) {
        Ok(text) if text.len() <= 1024 * 1024 => send(socket, Message::Text(text.into())).await,
        _ => false,
    }
}

impl Subscription {
    async fn run(mut self, mut socket: WebSocket, initial: MatchState) {
        if *self.shutdown.borrow() {
            return;
        }
        let mut turn = initial.turn;
        if !send_state(&mut socket, &initial).await {
            return;
        }
        if initial.terminated || initial.truncated {
            let _ = send(&mut socket, Message::Close(None)).await;
            return;
        }
        let mut reconcile = tokio::time::interval(Duration::from_secs(2));
        reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        reconcile.tick().await;
        loop {
            let refresh = tokio::select! {
                _ = self.shutdown.changed() => break,
                incoming = socket.recv() => {
                    match incoming {
                        None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                        Some(Ok(Message::Text(_) | Message::Binary(_))) => {
                            let _ = send(&mut socket, Message::Close(Some(CloseFrame { code: 1003, reason: "Submit actions through REST".into() }))).await;
                            return;
                        }
                        Some(Ok(_)) => {}
                    }
                    false
                },
                changed = self.changes.recv() => match changed {
                    Ok(id) => id == self.id,
                    Err(broadcast::error::RecvError::Lagged(_)) => true,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = reconcile.tick() => true,
            };
            if !refresh {
                continue;
            }
            let replay = tokio::select! {
                _ = self.shutdown.changed() => break,
                result = self.service.get_replay(&self.id, self.viewer) => result,
            };
            match replay {
                Ok(replay) => {
                    let previous_turn = turn;
                    for state in replay
                        .states
                        .iter()
                        .filter(|state| state.turn > previous_turn)
                    {
                        if *self.shutdown.borrow() || !send_state(&mut socket, state).await {
                            return;
                        }
                        turn = state.turn;
                        if state.terminated || state.truncated {
                            let _ = send(&mut socket, Message::Close(None)).await;
                            return;
                        }
                    }
                }
                Err(error) => {
                    let Ok(message) = serde_json::to_string(&StreamMessage::Error { error }) else {
                        break;
                    };
                    let _ = send(&mut socket, Message::Text(message.into())).await;
                    break;
                }
            }
        }
        let _ = send(&mut socket, Message::Close(None)).await;
    }
}

//! Local-mode REST adapter. Game rules and persistence remain in injected services.
//!
//! This adapter implements the PRD's auth-disabled local mode. It requires a loopback
//! listener, a loopback ConnectInfo peer, and a matching Host and optional Origin.
//! It does not issue seat credentials and must not be mounted behind a public proxy.
mod access;
mod error;
mod stream;

pub use stream::LiveUpdates;

use axum::{
    extract::{
        rejection::{JsonRejection, PathRejection, QueryRejection},
        DefaultBodyLimit, Path, Query, State,
    },
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use error::HttpError;
use gfa_api_types::{ApiError, CreateMatch, MatchState, MoveRequest, MoveResult, Replay};
use gfa_core::{GameSpec, Viewer};
use gfa_service::GameService;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};

/// Maximum JSON request size, including optional move reasoning.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// Construct the local API for the actual bound listener address.
///
/// Serve using `into_make_service_with_connect_info::<SocketAddr>()`; requests
/// without peer information are refused. The address must already be loopback.
pub fn local_router(service: Arc<GameService>, address: SocketAddr) -> Result<Router, ApiError> {
    build_router(service, address, None)
}

/// Construct local REST and WebSocket routes using the service's commit observer.
///
/// The host must install this same LiveUpdates instance with GameService::with_observer.
/// Periodic reconciliation also catches commits from other hosts sharing the store.
pub fn local_router_with_updates(
    service: Arc<GameService>,
    address: SocketAddr,
    updates: Arc<LiveUpdates>,
) -> Result<Router, ApiError> {
    build_router(service, address, Some(updates))
}

fn build_router(
    service: Arc<GameService>,
    address: SocketAddr,
    updates: Option<Arc<LiveUpdates>>,
) -> Result<Router, ApiError> {
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(ApiError::new(
            "INVALID_CONFIG",
            "A bound loopback listener is required",
            "Bind 127.0.0.1 or ::1, then pass listener.local_addr().",
        ));
    }
    let mut router = Router::new()
        .route("/healthz", get(health))
        .route("/v1/games", get(games))
        .route("/v1/games/{game_id}", get(game))
        .route("/v1/matches", post(create))
        .route("/v1/matches/{id}/state", get(state))
        .route("/v1/matches/{id}/legal-actions", get(legal_actions))
        .route("/v1/matches/{id}/actions", post(make_move))
        .route("/v1/matches/{id}/replay", get(replay))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed);
    if let Some(updates) = updates {
        router = router.route(
            "/v1/matches/{id}/stream",
            get(stream::upgrade).layer(axum::Extension(updates)),
        );
    }
    Ok(router
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            access::LocalAccess::new(address),
            access::guard,
        ))
        .with_state(service))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewQuery {
    seat: Option<u8>,
}

impl ViewQuery {
    fn viewer(self) -> Viewer {
        self.seat.map_or(Viewer::Spectator, Viewer::Player)
    }
}

type View = Result<Query<ViewQuery>, QueryRejection>;
type Id = Result<Path<String>, PathRejection>;

fn view(query: View) -> Result<ViewQuery, HttpError> {
    query
        .map(|Query(value)| value)
        .map_err(|error| HttpError::request(error.body_text()))
}

fn id(path: Id) -> Result<String, HttpError> {
    path.map(|Path(value)| value)
        .map_err(|error| HttpError::request(error.body_text()))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "mode": "local" }))
}

async fn games(State(service): State<Arc<GameService>>) -> Json<Vec<GameSpec>> {
    Json(service.list_games())
}

async fn game(
    State(service): State<Arc<GameService>>,
    path: Id,
) -> Result<Json<GameSpec>, HttpError> {
    let id = id(path)?;
    service
        .list_games()
        .into_iter()
        .find(|spec| spec.id == id)
        .map(Json)
        .ok_or_else(|| {
            ApiError::new(
                "UNKNOWN_GAME",
                "Game is not registered",
                "Choose a game returned by GET /v1/games.",
            )
            .into()
        })
}

async fn create(
    State(service): State<Arc<GameService>>,
    query: View,
    body: Result<Json<CreateMatch>, JsonRejection>,
) -> Result<Response, HttpError> {
    let viewer = Viewer::Player(view(query)?.seat.unwrap_or(0));
    let Json(request) = body?;
    let state = service.create_match(request, viewer).await?;
    Ok((StatusCode::CREATED, Json(state)).into_response())
}

async fn state(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: View,
) -> Result<Json<MatchState>, HttpError> {
    Ok(Json(
        service.get_state(&id(path)?, view(query)?.viewer()).await?,
    ))
}

async fn legal_actions(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: View,
) -> Result<Json<Value>, HttpError> {
    let state = service.get_state(&id(path)?, view(query)?.viewer()).await?;
    Ok(Json(
        json!({ "turn": state.turn, "to_act": state.to_act, "legal_actions": state.legal_actions, "action_mask": state.action_mask }),
    ))
}

async fn make_move(
    State(service): State<Arc<GameService>>,
    path: Id,
    headers: HeaderMap,
    body: Result<Json<MoveRequest>, JsonRejection>,
) -> Result<Json<MoveResult>, HttpError> {
    let id = id(path)?;
    if headers.get_all("idempotency-key").iter().count() > 1 {
        return Err(HttpError::request(
            "Provide at most one Idempotency-Key header",
        ));
    }
    let key = headers
        .get("idempotency-key")
        .map(|value| {
            value
                .to_str()
                .map_err(|_| HttpError::request("Idempotency-Key must be text"))
        })
        .transpose()?;
    let Json(request) = body?;
    Ok(Json(service.make_move(&id, request, key).await?))
}

async fn replay(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: View,
) -> Result<Json<Replay>, HttpError> {
    Ok(Json(
        service
            .get_replay(&id(path)?, view(query)?.viewer())
            .await?,
    ))
}

async fn not_found() -> HttpError {
    HttpError::new(
        StatusCode::NOT_FOUND,
        "ROUTE_NOT_FOUND",
        "Route does not exist",
        "Use a documented /v1 endpoint.",
    )
}

async fn method_not_allowed() -> HttpError {
    HttpError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "METHOD_NOT_ALLOWED",
        "HTTP method is not supported for this route",
        "Check the endpoint's documented HTTP method.",
    )
}

#[cfg(test)]
mod tests;

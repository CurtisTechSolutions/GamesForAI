//! Local-mode REST adapter. Game rules and persistence remain in injected services.
//!
//! This adapter implements the PRD's auth-disabled local mode. It requires a loopback
//! listener, a loopback ConnectInfo peer, and a matching Host and optional Origin.
//! It does not issue seat credentials and must not be mounted behind a public proxy.
mod access;
mod error;
mod events;
mod info;
mod openapi;
mod opponents;
pub use openapi::openapi_document;
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
#[cfg(test)]
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

/// Apply local peer, Host, Origin, and response-header policy to host-composed routes.
/// Additional transports remain responsible for enforcing their own request-body limit.
pub fn protect_local_routes(router: Router, address: SocketAddr) -> Result<Router, ApiError> {
    validate_local_address(address)?;
    Ok(router.layer(middleware::from_fn_with_state(access::LocalAccess::new(address), access::guard)))
}
fn validate_local_address(address: SocketAddr) -> Result<(),ApiError> {
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(ApiError::new(
            "INVALID_CONFIG",
            "A bound loopback listener is required",
            "Bind 127.0.0.1 or ::1, then pass listener.local_addr().",
        ));
    }
    Ok(())
}

fn build_router(
    service: Arc<GameService>,
    address: SocketAddr,
    updates: Option<Arc<LiveUpdates>>,
) -> Result<Router, ApiError> {
    validate_local_address(address)?;
    let document = openapi_document(updates.is_some()).to_json().map_err(|_| {
        ApiError::new(
            "INVALID_CONFIG",
            "API documentation could not be generated",
            "Check schema derives.",
        )
    })?;
    let docs = utoipa_swagger_ui::SwaggerUi::new("/docs").config(
        utoipa_swagger_ui::Config::from("/v1/openapi.json")
            .validator_url("none")
            .query_config_enabled(false)
            .use_base_layout()
            .filter(true)
            .try_it_out_enabled(true),
    );
    let mut router = Router::new()
        .merge(docs)
        .route(
            "/v1/openapi.json",
            get(openapi::document).layer(axum::Extension(openapi::Document(document))),
        )
        .route("/healthz", get(health))
        .route("/v1/games", get(games))
        .route("/v1/games/{game_id}", get(game))
        .route("/v1/games/{game_id}/info", get(info::game_info))
        .route("/v1/games/{game_id}/simulate", post(simulate))
        .route("/v1/games/{game_id}/opponents", get(opponents::catalog))
        .route("/v1/analysis", post(opponents::analyze))
        .route(
            "/v1/games/{game_id}/positions/validate",
            post(validate_position),
        )
        .route("/v1/matches", post(create).get(history))
        .route("/v1/matches/{id}", get(metadata))
        .route("/v1/matches/{id}/resign", post(resign))
        .route("/v1/matches/{id}/fork", post(fork_match))
        .route("/v1/matches/{id}/offer-draw", post(offer_draw))
        .route("/v1/matches/{id}/info", get(info::match_info))
        .route("/v1/matches/{id}/state", get(state))
        .route("/v1/matches/{id}/legal-actions", get(legal_actions))
        .route("/v1/matches/{id}/actions", post(make_move))
        .route("/v1/matches/{id}/replay", get(replay))
        .route("/v1/matches/{id}/events", get(events::history))
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

#[utoipa::path(
    get, path = "/healthz", tag = "Health",


    responses((status = 200, description = "Successful health response", body = gfa_api_types::Health), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
async fn health() -> Json<gfa_api_types::Health> {
    Json(gfa_api_types::Health {
        status: "ok".into(),
        mode: "local".into(),
    })
}

#[utoipa::path(
    get, path = "/v1/games", tag = "Games",


    responses((status = 200, description = "Successful games response", body = Vec<GameSpec>), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
async fn games(State(service): State<Arc<GameService>>) -> Json<Vec<GameSpec>> {
    Json(service.list_games())
}

#[utoipa::path(
    get, path = "/v1/games/{game_id}", tag = "Games",

    params(("game_id" = String, Path, description = "Registered game identifier")),
    responses((status = 200, description = "Successful game response", body = GameSpec), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
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

#[utoipa::path(
    post, path = "/v1/matches", tag = "Matches",
    request_body(content = gfa_api_types::CreateMatch, example = json!({"game_id":"tictactoe","seed":42})),
    params(("seat" = Option<u8>, Query, description = "Zero-based seat; omit for spectator view. Creation and position validation default to seat 0.", minimum = 0, maximum = 255)),
    responses((status = 201, description = "Successful create response", body = gfa_api_types::CreatedMatch), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
async fn create(
    State(service): State<Arc<GameService>>,
    query: View,
    body: Result<Json<CreateMatch>, JsonRejection>,
) -> Result<Response, HttpError> {
    let viewer = Viewer::Player(view(query)?.seat.unwrap_or(0));
    let Json(request) = body?;
    let state = service.create_match_with_info(request, viewer).await?;
    Ok((StatusCode::CREATED, Json(state)).into_response())
}

#[utoipa::path(
    get, path = "/v1/matches/{id}/state", tag = "Matches",

    params(("id" = String, Path, description = "Match identifier"),("seat" = Option<u8>, Query, description = "Zero-based seat; omit for spectator view. Creation and position validation default to seat 0.", minimum = 0, maximum = 255)),
    responses((status = 200, description = "Successful state response", body = MatchState), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
async fn state(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: View,
) -> Result<Json<MatchState>, HttpError> {
    Ok(Json(
        service.get_state(&id(path)?, view(query)?.viewer()).await?,
    ))
}

#[utoipa::path(
    get, path = "/v1/matches/{id}/legal-actions", tag = "Matches",

    params(("id" = String, Path, description = "Match identifier"),("seat" = Option<u8>, Query, description = "Zero-based seat; omit for spectator view. Creation and position validation default to seat 0.", minimum = 0, maximum = 255)),
    responses((status = 200, description = "Successful legal actions response", body = gfa_api_types::LegalActions), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
async fn legal_actions(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: View,
) -> Result<Json<gfa_api_types::LegalActions>, HttpError> {
    let state = service.get_state(&id(path)?, view(query)?.viewer()).await?;
    Ok(Json(gfa_api_types::LegalActions {
        turn: state.turn,
        to_act: state.to_act,
        legal_actions: state.legal_actions,
        action_mask: state.action_mask,
    }))
}

#[utoipa::path(
    post, path = "/v1/matches/{id}/actions", tag = "Matches",
    request_body(content = MoveRequest, example = json!({"seat":0,"turn":0,"action":"r2c2"})),
    params(("id" = String, Path, description = "Match identifier"),("Idempotency-Key" = Option<String>, Header, description = "1–128 bytes; identical retries return the original result.", min_length = 1, max_length = 128)),
    responses((status = 200, description = "Successful make move response", body = MoveResult), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
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

#[utoipa::path(
    get, path = "/v1/matches/{id}/replay", tag = "Matches",

    params(("id" = String, Path, description = "Match identifier"),("seat" = Option<u8>, Query, description = "Zero-based seat; omit for spectator view. Creation and position validation default to seat 0.", minimum = 0, maximum = 255)),
    responses((status = 200, description = "Successful replay response", body = Replay), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
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

#[utoipa::path(
    post, path = "/v1/games/{game_id}/positions/validate", tag = "Positions",
    request_body = gfa_api_types::ValidatePosition,
    params(("game_id" = String, Path, description = "Registered game identifier"),("seat" = Option<u8>, Query, description = "Zero-based seat; omit for spectator view. Creation and position validation default to seat 0.", minimum = 0, maximum = 255)),
    responses((status = 200, description = "Successful validate position response", body = gfa_api_types::ValidatedPosition), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
async fn validate_position(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: View,
    body: Result<Json<gfa_api_types::ValidatePosition>, JsonRejection>,
) -> Result<Json<gfa_api_types::ValidatedPosition>, HttpError> {
    let Json(request) = body?;
    let viewer = Viewer::Player(view(query)?.seat.unwrap_or(0));
    Ok(Json(service.validate_position(
        &id(path)?,
        request,
        viewer,
    )?))
}

#[utoipa::path(
    get, path = "/v1/matches", tag = "Matches",
    params(gfa_api_types::MatchHistoryQuery),
    responses((status = 200, description = "Bounded local history, ordered by match id. Follow next even when filters return an empty page.", body = gfa_api_types::MatchHistory),
        (status = "default", description = "Query or service error", body = gfa_api_types::ErrorResponse))
)]
async fn history(
    State(service): State<Arc<GameService>>,
    query: Result<Query<gfa_api_types::MatchHistoryQuery>, QueryRejection>,
) -> Result<Json<gfa_api_types::MatchHistory>, HttpError> {
    let Query(query) = query.map_err(|error| HttpError::request(error.body_text()))?;
    Ok(Json(service.list_matches(query).await?))
}

#[utoipa::path(
    get, path = "/v1/matches/{id}", tag = "Matches",
    params(("id" = String, Path, description = "Match identifier")),
    responses((status = 200, description = "Public match metadata without hidden state or RNG", body = gfa_api_types::MatchMetadata),
        (status = "default", description = "Match or service error", body = gfa_api_types::ErrorResponse))
)]
async fn metadata(
    State(service): State<Arc<GameService>>,
    path: Id,
) -> Result<Json<gfa_api_types::MatchMetadata>, HttpError> {
    Ok(Json(service.get_match(&id(path)?).await?))
}

#[utoipa::path(
    post, path = "/v1/matches/{id}/resign", tag = "Matches",
    params(("id" = String, Path, description = "Match identifier")),
    request_body = gfa_api_types::ControlRequest,
    responses((status = 200, description = "Resign a one- or two-seat match.", body = MatchState),
        (status = "default", description = "Control or turn error", body = gfa_api_types::ErrorResponse))
)]
async fn resign(
    State(service): State<Arc<GameService>>,
    path: Id,
    body: Result<Json<gfa_api_types::ControlRequest>, JsonRejection>,
) -> Result<Json<MatchState>, HttpError> {
    let Json(request) = body?;
    Ok(Json(service.resign(&id(path)?, request).await?))
}

#[utoipa::path(
    post, path = "/v1/matches/{id}/offer-draw", tag = "Matches",
    params(("id" = String, Path, description = "Match identifier")),
    request_body = gfa_api_types::ControlRequest,
    responses((status = 200, description = "Offer a draw or accept the other seat’s pending offer. A move expires the offer.", body = MatchState),
        (status = "default", description = "Control or turn error", body = gfa_api_types::ErrorResponse))
)]
async fn offer_draw(
    State(service): State<Arc<GameService>>,
    path: Id,
    body: Result<Json<gfa_api_types::ControlRequest>, JsonRejection>,
) -> Result<Json<MatchState>, HttpError> {
    let Json(request) = body?;
    Ok(Json(service.offer_draw(&id(path)?, request).await?))
}

#[utoipa::path(
    post, path = "/v1/games/{game_id}/simulate", tag = "Games",
    params(("game_id" = String, Path, description = "Registered game identifier"),
        ("seat" = Option<u8>, Query, description = "Standalone viewer, default 0. Must match the body seat for a match source.")),
    request_body = gfa_api_types::SimulateRequest,
    responses((status = 200, description = "Independent hypothetical lines with recoverable per-line failures", body = gfa_api_types::SimulationResult),
        (status = "default", description = "Assist policy or source error", body = gfa_api_types::ErrorResponse))
)]
async fn simulate(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: View,
    body: Result<Json<gfa_api_types::SimulateRequest>, JsonRejection>,
) -> Result<Json<gfa_api_types::SimulationResult>, HttpError> {
    let Json(request) = body?;
    let seat = view(query)?.seat.unwrap_or(match &request.from {
        gfa_api_types::SimulationFrom::Match { seat, .. } => *seat,
        _ => 0,
    });
    Ok(Json(
        service
            .simulate(&id(path)?, request, Viewer::Player(seat))
            .await?,
    ))
}

#[utoipa::path(
    post, path = "/v1/matches/{id}/fork", tag = "Matches",
    params(("id" = String, Path, description = "Parent match identifier"),
        ("seat" = Option<u8>, Query, description = "Authorized local seat, default 0", minimum = 0, maximum = 255)),
    request_body = gfa_api_types::ForkMatch,
    responses((status = 201, description = "Independent variation with optional briefing", body = gfa_api_types::CreatedMatch),
        (status = "default", description = "Invalid turn or fork permission", body = gfa_api_types::ErrorResponse))
)]
async fn fork_match(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: View,
    body: Result<Json<gfa_api_types::ForkMatch>, JsonRejection>,
) -> Result<Response, HttpError> {
    let viewer = Viewer::Player(view(query)?.seat.unwrap_or(0));
    let Json(request) = body?;
    // This router is restricted to the single local owner by its access layer.
    // Public transports must derive ownership and full-state access from auth.
    let result = service
        .fork_match(
            &id(path)?,
            request,
            gfa_service::ForkAccess {
                viewer,
                owns_parent: true,
                full_state: false,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(result)).into_response())
}

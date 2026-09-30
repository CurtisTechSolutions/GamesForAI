use super::{id, view, HttpError, Id, View};
use axum::{extract::{rejection::JsonRejection, State}, Json};
use gfa_api_types::{AnalysisRequest, AnalysisResult, OpponentSpec, SimulationFrom};
use gfa_core::Viewer;
use gfa_service::GameService;
use std::sync::Arc;

#[utoipa::path(
    get, path = "/v1/games/{game_id}/opponents", tag = "Games",
    params(("game_id" = String, Path, description = "Registered game identifier")),
    responses((status = 200, description = "Installed opponents and measured calibration, when available", body = [OpponentSpec]),
        (status = "default", description = "Unknown game", body = gfa_api_types::ErrorResponse))
)]
pub(super) async fn catalog(State(service): State<Arc<GameService>>, path: Id) -> Result<Json<Vec<OpponentSpec>>, HttpError> {
    Ok(Json(service.list_opponents(&id(path)?)?))
}

#[utoipa::path(
    post, path = "/v1/analysis", tag = "Analysis",
    params(("seat" = Option<u8>, Query, description = "Authorized seat; defaults to the match source seat or zero")),
    request_body = AnalysisRequest,
    responses((status = 200, description = "Bounded engine recommendation with search diagnostics", body = AnalysisResult),
        (status = "default", description = "Invalid position, assist policy, or unavailable worker", body = gfa_api_types::ErrorResponse))
)]
pub(super) async fn analyze(
    State(service): State<Arc<GameService>>, query: View,
    body: Result<Json<AnalysisRequest>, JsonRejection>,
) -> Result<Json<AnalysisResult>, HttpError> {
    let Json(request) = body?;
    let seat = view(query)?.seat.unwrap_or(match &request.from {
        SimulationFrom::Match { seat, .. } => *seat,
        _ => 0,
    });
    Ok(Json(service.analyze(request, Viewer::Player(seat)).await?))
}

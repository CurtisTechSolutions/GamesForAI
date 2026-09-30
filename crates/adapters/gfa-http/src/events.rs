use super::{id, HttpError, Id};
use axum::{extract::{rejection::QueryRejection, Query, State}, Json};
use gfa_api_types::{EventPage, EventsQuery};
use gfa_core::Viewer;
use gfa_service::GameService;
use std::sync::Arc;

#[utoipa::path(
    get, path = "/v1/matches/{id}/events", tag = "Matches",
    params(("id" = String, Path, description = "Match identifier"), EventsQuery),
    responses((status = 200, description = "Consecutive events projected to one authorized viewer", body = EventPage),
        (status = "default", description = "Invalid cursor or unavailable history", body = gfa_api_types::ErrorResponse))
)]
pub(super) async fn history(
    State(service): State<Arc<GameService>>, path: Id,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> Result<Json<EventPage>, HttpError> {
    let Query(query) = query.map_err(|e| HttpError::request(e.body_text()))?;
    let viewer = query.seat.map_or(Viewer::Spectator, Viewer::Player);
    Ok(Json(service.get_events(&id(path)?, query, viewer).await?))
}

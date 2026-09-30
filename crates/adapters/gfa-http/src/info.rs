use crate::{error::HttpError, id, Id};
use axum::{
    extract::{rejection::QueryRejection, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use gfa_api_types::{
    Briefing, GameInfoQuery as GameQuery, InfoFormat as Format, MatchInfoQuery as MatchQuery,
};
use gfa_service::GameService;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[utoipa::path(
    get, path = "/v1/games/{game_id}/info", tag = "Briefings",

    params(("game_id" = String, Path, description = "Registered game identifier"),gfa_api_types::GameInfoQuery, ("If-None-Match" = Option<String>, Header, description = "ETag, weak tag, list, or *")),
    responses((status = 200, description = "Deterministic synthetic briefing", content((Briefing = "application/json"), (String = "text/markdown")), headers(("ETag" = String, description = "SHA-256 of this representation"))), (status = 304, description = "Unchanged; no body"), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
pub(crate) async fn game_info(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: Result<Query<GameQuery>, QueryRejection>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let Query(query) = query.map_err(|error| HttpError::request(error.body_text()))?;
    let config = match query.config {
        Some(text) => {
            if text.len() > 8192 {
                return Err(HttpError::request("Configuration exceeds 8192 bytes"));
            }
            serde_json::from_str(&text)
                .map_err(|_| HttpError::request("config must be valid JSON"))?
        }
        None => Value::Null,
    };
    let info = service.get_game_info(&id(path)?, &config, query.seat, query.detail)?;
    render(&info, query.format, Some(&headers))
}

pub(crate) fn render(
    info: &Briefing,
    format: Format,
    cache: Option<&HeaderMap>,
) -> Result<Response, HttpError> {
    let (content_type, body) = match format {
        Format::Json => ("application/json", serde_json::to_string(info)),
        Format::Markdown => ("text/markdown; charset=utf-8", info.markdown()),
    };
    let body = body.map_err(|_| {
        HttpError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INVALID_BRIEFING",
            "Briefing serialization failed",
            "Check the engine's briefing data.",
        )
    })?;
    let etag = format!("\"{:x}\"", Sha256::digest(body.as_bytes()));
    let matched = cache.is_some_and(|headers| {
        headers
            .get_all(header::IF_NONE_MATCH)
            .iter()
            .filter_map(|header| header.to_str().ok())
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag)
    });
    let mut response = if matched {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, content_type)], body).into_response()
    };
    if cache.is_some() {
        response.headers_mut().insert(
            header::ETAG,
            etag.parse()
                .map_err(|_| HttpError::request("Invalid generated ETag"))?,
        );
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            "private, max-age=0, must-revalidate"
                .parse()
                .map_err(|_| HttpError::request("Invalid cache policy"))?,
        );
    }
    Ok(response)
}

#[utoipa::path(
    get, path = "/v1/matches/{id}/info", tag = "Briefings",

    params(("id" = String, Path, description = "Match identifier"),gfa_api_types::MatchInfoQuery),
    responses((status = 200, description = "Live match briefing; never cached", content((Briefing = "application/json"), (String = "text/markdown"))), (status = "default", description = "Structured recoverable error; local access requires a loopback peer and matching Host/Origin.", body = gfa_api_types::ErrorResponse))
)]
pub(crate) async fn match_info(
    State(service): State<Arc<GameService>>,
    path: Id,
    query: Result<Query<MatchQuery>, QueryRejection>,
) -> Result<Response, HttpError> {
    let Query(query) = query.map_err(|error| HttpError::request(error.body_text()))?;
    let viewer = query
        .seat
        .map_or(gfa_core::Viewer::Spectator, gfa_core::Viewer::Player);
    let info = service
        .get_match_info(&id(path)?, viewer, query.detail)
        .await?;
    // Live/private match information is never cached or served conditionally.
    render(&info, query.format, None)
}

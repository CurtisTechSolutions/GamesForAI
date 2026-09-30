use crate::{error::HttpError, id, Id};
use axum::{
    extract::{rejection::QueryRejection, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use gfa_api_types::{Briefing, InfoDetail};
use gfa_service::GameService;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Format {
    #[default]
    Json,
    Markdown,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GameQuery {
    #[serde(default)]
    format: Format,
    #[serde(default)]
    detail: InfoDetail,
    config: Option<String>,
    seat: Option<u8>,
}

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
            serde_json::from_str(&text).map_err(|_| HttpError::request("config must be valid JSON"))?
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
    let body = body.map_err(|_| HttpError::new(
        StatusCode::INTERNAL_SERVER_ERROR, "INVALID_BRIEFING",
        "Briefing serialization failed", "Check the engine's briefing data.",
    ))?;
    let etag = format!("\"{:x}\"", Sha256::digest(body.as_bytes()));
    let matched = cache.is_some_and(|headers| headers.get_all(header::IF_NONE_MATCH).iter()
        .filter_map(|header|header.to_str().ok())
        .flat_map(|value|value.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag));
    let mut response = if matched {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, content_type)], body).into_response()
    };
    if cache.is_some() {
        response.headers_mut().insert(header::ETAG, etag.parse().map_err(|_| HttpError::request("Invalid generated ETag"))?);
        response.headers_mut().insert(header::CACHE_CONTROL, "private, max-age=0, must-revalidate".parse().map_err(|_| HttpError::request("Invalid cache policy"))?);
    }
    Ok(response)
}

use axum::{http::header, response::IntoResponse, Extension};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "GamesForAI local API", description = "Single-user local play. Requests require a loopback peer and matching Host and optional Origin. Any local caller can control either seat. Bodies are limited to 64 KiB. Only implemented endpoints are listed."),
    servers((url = "/", description = "This local host")),
    paths(
        super::resign, super::offer_draw, super::history, super::metadata, super::health, super::games, super::game, super::create, super::state,
        super::legal_actions, super::make_move, super::replay, super::validate_position,
        super::info::game_info, super::info::match_info, super::stream::upgrade, document
    ),
    components(schemas(gfa_api_types::GameInfoQuery, gfa_api_types::MatchInfoQuery, gfa_api_types::StreamMessage))
)]
struct ApiDoc;

/// Generate the installed local API contract directly from its Rust DTOs.
///
/// Set include_streams to false for a REST-only router without LiveUpdates.
pub fn openapi_document(include_streams: bool) -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    if !include_streams {
        document.paths.paths.remove("/v1/matches/{id}/stream");
    }
    document
}

#[derive(Clone)]
pub(crate) struct Document(pub String);

#[utoipa::path(
    get, path = "/v1/openapi.json", tag = "Documentation",
    responses((status = 200, description = "OpenAPI 3.1 document for this router", body = Object),
        (status = "default", description = "Local access error", body = gfa_api_types::ErrorResponse))
)]
pub(crate) async fn document(Extension(document): Extension<Document>) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json")], document.0)
}

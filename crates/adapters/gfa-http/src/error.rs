use axum::{
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use gfa_api_types::{ApiError, ErrorResponse};

pub(crate) struct HttpError {
    pub status: StatusCode,
    pub error: ApiError,
}

impl HttpError {
    pub fn request(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            message,
            "Check the request fields and URL parameters.",
        )
    }

    pub fn new(status: StatusCode, code: &str, message: impl Into<String>, hint: &str) -> Self {
        Self {
            status,
            error: ApiError::new(code, message, hint),
        }
    }
}

impl From<ApiError> for HttpError {
    fn from(error: ApiError) -> Self {
        let status = match error.code.as_str() {
            "UNKNOWN_GAME" | "MATCH_NOT_FOUND" => StatusCode::NOT_FOUND,
            "STALE_TURN" | "IDEMPOTENCY_CONFLICT" | "MATCH_FINISHED" | "ID_CONFLICT" => {
                StatusCode::CONFLICT
            }
            "UNAUTHORIZED" | "ASSIST_NOT_ALLOWED" | "FORBIDDEN" => StatusCode::FORBIDDEN,
            "ENGINE_UNAVAILABLE" | "STORAGE_UNAVAILABLE" => StatusCode::SERVICE_UNAVAILABLE,
            "INVALID_EVENT_LOG" | "INVALID_BRIEFING" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        Self { status, error }
    }
}

impl From<JsonRejection> for HttpError {
    fn from(error: JsonRejection) -> Self {
        Self::new(error.status(), "INVALID_REQUEST", error.body_text(), "Send a JSON body matching the documented request fields with Content-Type: application/json.")
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorResponse { error: self.error })).into_response()
    }
}

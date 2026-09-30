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
            "ENGINE_UNAVAILABLE" | "ENGINE_BUSY" | "ENGINE_CANCELLED" | "STORAGE_UNAVAILABLE" => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            "ENGINE_TIMEOUT" => StatusCode::GATEWAY_TIMEOUT,
            "ENGINE_INVALID_RESPONSE" => StatusCode::BAD_GATEWAY,
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_failure_statuses_are_not_player_action_errors() {
        for (code, status) in [
            ("ENGINE_BUSY", StatusCode::SERVICE_UNAVAILABLE),
            ("ENGINE_UNAVAILABLE", StatusCode::SERVICE_UNAVAILABLE),
            ("ENGINE_CANCELLED", StatusCode::SERVICE_UNAVAILABLE),
            ("ENGINE_TIMEOUT", StatusCode::GATEWAY_TIMEOUT),
            ("ENGINE_INVALID_RESPONSE", StatusCode::BAD_GATEWAY),
            ("ILLEGAL_ACTION", StatusCode::UNPROCESSABLE_ENTITY),
        ] {
            let error = HttpError::from(ApiError::new(code, "failure", "retry"));
            assert_eq!(error.status, status);
            assert_eq!(error.error.code, code);
        }
    }
}

use crate::StoreError;
use gfa_api_types::ApiError;
use gfa_core::{ErrorCode, GameError};

pub(crate) fn engine(error: GameError) -> ApiError {
    let code = match error.code {
        ErrorCode::UnknownGame => "UNKNOWN_GAME",
        ErrorCode::InvalidConfig => "INVALID_CONFIG",
        ErrorCode::InvalidPosition => "INVALID_POSITION",
        ErrorCode::NotYourTurn => "NOT_YOUR_TURN",
        ErrorCode::IllegalAction => "ILLEGAL_ACTION",
        ErrorCode::UnparseableAction => "UNPARSEABLE_ACTION",
        ErrorCode::MatchFinished => "MATCH_FINISHED",
        ErrorCode::Serialization => "INVALID_POSITION",
    };
    ApiError::new(code, error.message, &error.hint)
}

pub(crate) fn store(error: StoreError) -> ApiError {
    match error {
        StoreError::Conflict => ApiError::new(
            "STALE_TURN",
            "Another command changed this match",
            "Read the current state and retry with its turn.",
        ),
        StoreError::NotFound => missing(),
        StoreError::DuplicateId => ApiError::new(
            "ID_CONFLICT",
            "Match identifier collision",
            "Retry creating the match.",
        ),
        StoreError::IdempotencyConflict => ApiError::new(
            "IDEMPOTENCY_CONFLICT",
            "This idempotency key was used for different input",
            "Retry the original request or use a new key.",
        ),
        StoreError::Unavailable(_) => ApiError::new(
            "STORAGE_UNAVAILABLE",
            "Match storage is unavailable",
            "Retry later using the same idempotency key.",
        ),
    }
}

pub(crate) fn missing() -> ApiError {
    ApiError::new(
        "MATCH_NOT_FOUND",
        "Match does not exist",
        "Check the match identifier.",
    )
}

pub(crate) fn corrupt() -> ApiError {
    ApiError::new(
        "INVALID_EVENT_LOG",
        "Stored events cannot be replayed consistently",
        "Use the engine version recorded with this match and check the event log.",
    )
}

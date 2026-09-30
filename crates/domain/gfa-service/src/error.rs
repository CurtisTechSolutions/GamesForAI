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


pub(crate) fn opponent(error: gfa_core::OpponentError) -> ApiError {
    use gfa_core::OpponentError;
    let (code, message, hint) = match error {
        OpponentError::Game(error) => return engine(error),
        OpponentError::Busy => ("ENGINE_BUSY", "All opponent workers are busy", "Retry after a running request finishes."),
        OpponentError::Timeout => ("ENGINE_TIMEOUT", "Opponent exceeded its time budget", "Increase the move budget or select another opponent."),
        OpponentError::Cancelled => ("ENGINE_CANCELLED", "Opponent request was cancelled", "Read the current match state before retrying."),
        OpponentError::BudgetExhausted => ("BUDGET_EXHAUSTED", "Opponent usage budget is exhausted", "Change the configured budget before requesting another decision."),
        OpponentError::InvalidResponse => ("ENGINE_INVALID_RESPONSE", "Opponent returned an invalid recommendation", "Select another opponent and report the provider failure."),
        _ => ("ENGINE_UNAVAILABLE", "Opponent is unavailable", "Check the configured provider or installed engine."),
    };
    ApiError::new(code, message, hint)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operational_failures_keep_their_identity() {
        use gfa_core::OpponentError;
        for (input, code) in [
            (OpponentError::Busy, "ENGINE_BUSY"),
            (OpponentError::Unavailable, "ENGINE_UNAVAILABLE"),
            (OpponentError::Timeout, "ENGINE_TIMEOUT"),
            (OpponentError::Cancelled, "ENGINE_CANCELLED"),
            (OpponentError::BudgetExhausted, "BUDGET_EXHAUSTED"),
            (OpponentError::InvalidResponse, "ENGINE_INVALID_RESPONSE"),
            (OpponentError::Game(GameError::illegal("bad input")), "ILLEGAL_ACTION"),
        ] {
            assert_eq!(opponent(input).code, code);
        }
    }
}

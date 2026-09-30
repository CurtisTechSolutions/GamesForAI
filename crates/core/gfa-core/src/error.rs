use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable engine errors, translated into API errors by the domain layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// An unknown game identifier.
    UnknownGame,
    /// Configuration cannot be used by this engine.
    InvalidConfig,
    /// A position is malformed or unreachable.
    InvalidPosition,
    /// The selected player cannot act now.
    NotYourTurn,
    /// The action parses but is illegal.
    IllegalAction,
    /// The action encoding cannot be parsed.
    UnparseableAction,
    /// The game already ended.
    MatchFinished,
    /// Internal serialization failed.
    Serialization,
}

/// Recoverable engine failure. Rejected actions must leave state unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[error("{message}")]
pub struct GameError {
    /// Machine-readable code.
    pub code: ErrorCode,
    /// Explanation of the failure.
    pub message: String,
    /// Instructions for recovering.
    pub hint: String,
}

impl GameError {
    /// Construct an error with recovery advice.
    pub fn new(code: ErrorCode, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.into(),
        }
    }

    /// Reject an invalid action with a canonical recovery instruction.
    pub fn illegal(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::IllegalAction,
            message,
            "Choose one of the current legal actions.",
        )
    }

    /// Reject a malformed or unreachable position.
    pub fn position(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::InvalidPosition,
            message,
            "Import a valid position using this game's published notation.",
        )
    }
}

impl From<serde_json::Error> for GameError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(
            ErrorCode::Serialization,
            error.to_string(),
            "Check the published JSON schema.",
        )
    }
}

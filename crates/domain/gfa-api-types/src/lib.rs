//! Transport-independent types for the initial match lifecycle.
use gfa_core::{LegalAction, Observation, PlayerId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A validated custom starting position.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Start {
    /// Game-specific position notation.
    Position {
        /// The engine's position string.
        position: String,
    },
    /// Complete state for in-process clients and future transports.
    State {
        /// Structured state validated by the engine.
        state: Value,
    },
}

fn include_info_by_default() -> bool {
    true
}

fn empty_config() -> Value {
    serde_json::json!({})
}

/// Create a match with external players. Opponent scheduling is a separate use case.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMatch {
    /// Stable registered game id.
    pub game_id: String,
    /// Validated game options.
    #[serde(default = "empty_config")]
    pub config: Value,
    /// Fixed seed, or a fresh seed supplied by the host.
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(minimum = 0))]
    pub seed: Option<u64>,
    /// Optional custom position.
    #[serde(default)]
    pub start: Option<Start>,
    /// Include a compact briefing in transport creation responses.
    #[serde(default = "include_info_by_default")]
    pub include_info: bool,
}

/// Submit one action against an expected turn.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveRequest {
    /// Acting seat.
    #[cfg_attr(feature = "openapi", schema(value_type = u8, minimum = 0, maximum = 255))]
    pub seat: PlayerId,
    /// Expected accepted-action count.
    #[cfg_attr(feature = "openapi", schema(minimum = 0))]
    pub turn: u64,
    /// Canonical string, structured action, or {"index": n}.
    pub action: Value,
    /// Optional explanation supplied by the player.
    #[serde(default)]
    pub reasoning: Option<String>,
}

/// Full state projected into an explicitly selected viewer's information set.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MatchState {
    /// Stable match identifier.
    pub match_id: String,
    /// Accepted action count, starting at zero.
    #[cfg_attr(feature = "openapi", schema(minimum = 0))]
    pub turn: u64,
    /// Seats that must act.
    pub to_act: Vec<PlayerId>,
    /// Viewer-scoped board.
    pub observation: Observation,
    /// Current moves for the requesting player, empty for spectators.
    pub legal_actions: Vec<LegalAction>,
    /// Fixed-size mask for the requesting player.
    pub action_mask: Vec<bool>,
    /// Current per-player returns.
    pub returns: Vec<f64>,
    /// The rules ended play.
    pub terminated: bool,
    /// A length or other external cap ended play.
    pub truncated: bool,
}

/// One accepted action and the resulting view.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MoveResult {
    /// Canonical encoding selected from the engine's legal actions.
    pub accepted_action: LegalAction,
    /// State for the acting player.
    pub state: MatchState,
}

/// Reconstructed observations, including the initial state at turn zero.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Replay {
    /// Stable match identifier.
    pub match_id: String,
    /// Engine version used to reconstruct this replay.
    pub engine_version: String,
    /// One viewer-scoped state per turn.
    pub states: Vec<MatchState>,
}

/// Stable, recoverable domain failure; transports wrap it under an error field.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    /// Machine-readable code.
    pub code: String,
    /// Explanation of the failure.
    pub message: String,
    /// Plain-English recovery advice.
    pub hint: String,
    /// Retry context, such as current turn and legal actions.
    pub details: Value,
}

impl ApiError {
    /// Construct an error without extra details.
    pub fn new(code: &str, message: impl Into<String>, hint: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            hint: hint.into(),
            details: Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn creation_defaults_and_custom_starts_are_unambiguous() -> Result<(), serde_json::Error> {
        let request: CreateMatch = serde_json::from_value(json!({"game_id":"tictactoe"}))?;
        assert_eq!(request.config, json!({}));
        assert!(request.seed.is_none());
        assert!(request.start.is_none());
        assert!(request.include_info);
        assert!(serde_json::from_value::<Start>(json!({"position":"x","state":{}})).is_err());
        assert!(
            serde_json::from_value::<CreateMatch>(json!({"game_id":"tictactoe","typo":true}))
                .is_err()
        );
        let custom = CreateMatch {
            start: Some(Start::State {
                state: json!([1, 2, 3]),
            }),
            ..request
        };
        assert_eq!(
            serde_json::from_value::<CreateMatch>(serde_json::to_value(&custom)?)?,
            custom
        );
        Ok(())
    }

    #[test]
    fn move_requires_an_unsigned_seat_and_expected_turn() {
        for value in [
            json!({"seat":0,"action":"r1c1"}),
            json!({"seat":-1,"turn":0,"action":"r1c1"}),
            json!({"seat":0,"turn":-1,"action":"r1c1"}),
            json!({"seat":0,"turn":0,"action":"r1c1","typo":true}),
        ] {
            assert!(serde_json::from_value::<MoveRequest>(value).is_err());
        }
    }
}

mod info;
pub use info::{Briefing, GameInfoQuery, InfoDetail, InfoFormat, InfoSection, MatchInfoQuery};

/// Match creation response with the existing state fields and optional briefing.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CreatedMatch {
    /// Initial observation and legal actions, kept at the response's top level.
    #[serde(flatten)]
    pub state: MatchState,
    /// Compact match briefing; absent when include_info was false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<Briefing>,
}

mod position;
pub use position::{ValidatePosition, ValidatedPosition};

/// Local host liveness response.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Health {
    /// Liveness marker; currently ok.
    pub status: String,
    /// Serving mode; currently local.
    pub mode: String,
}

/// Current legal actions for one viewer.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LegalActions {
    /// Current expected turn.
    pub turn: u64,
    /// Seats allowed to act.
    pub to_act: Vec<PlayerId>,
    /// Canonical actions for the selected player.
    pub legal_actions: Vec<LegalAction>,
    /// Discrete legal mask.
    pub action_mask: Vec<bool>,
}

/// Stable error envelope shared by every REST failure.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ErrorResponse {
    /// Recoverable failure with retry context.
    pub error: ApiError,
}

/// JSON messages emitted by the read-only live state stream.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamMessage {
    /// Initial or newly committed viewer-scoped state.
    State {
        /// State visible to this connection.
        state: MatchState,
    },
    /// Recoverable service failure followed by stream closure.
    Error {
        /// Failure context.
        error: ApiError,
    },
}

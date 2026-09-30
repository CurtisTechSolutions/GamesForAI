use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Zero-based player seat.
pub type PlayerId = u8;

/// Who is allowed to see an observation.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "seat", rename_all = "snake_case")]
pub enum Viewer {
    /// One player's information set.
    Player(PlayerId),
    /// Public information only.
    Spectator,
    /// Full information; the service must authorize this view.
    Omniscient,
}

/// Turn scheduling declared by the engine.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnStructure {
    /// One player at a time.
    Sequential,
    /// Several players submit actions for the same turn.
    Simultaneous,
}

/// Visibility of the game's state.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Information {
    /// Every player sees the whole board.
    Perfect,
    /// Some state is private.
    Imperfect,
}

/// Static, machine-readable engine contract.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GameSpec {
    /// Stable game identifier.
    pub id: String,
    /// Human-readable title.
    pub name: String,
    /// Concise description.
    pub summary: String,
    /// Version needed for reproducible replay.
    pub engine_version: String,
    /// Version of the model briefing.
    pub info_version: String,
    /// Minimum and maximum seat counts.
    pub num_players: [u8; 2],
    /// Player labels, in seat order.
    pub seat_names: Vec<String>,
    /// Turn scheduling model.
    pub turn_structure: TurnStructure,
    /// Information available to players.
    pub information: Information,
    /// Whether chance affects transitions.
    pub stochastic: bool,
    /// Engine-enforced bound on actions before termination or truncation.
    pub max_game_length: u32,
    /// Minimum and maximum return.
    pub reward_range: [f64; 2],
    /// Size of the fixed discrete action space.
    pub action_space_size: u32,
    /// Complete rules, notation and worked examples.
    pub rules_markdown: String,
    /// Canonical notation grammar.
    pub action_notation: String,
    /// Position import/export description.
    pub position_notation: String,
    /// Generated configuration schema.
    pub config_schema: Value,
    /// Generated structured action schema.
    pub action_schema: Value,
    /// Generated observation schema.
    pub observation_schema: Value,
}

/// Optional fixed-shape neural-network input.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tensor {
    /// Dimensions in row-major order.
    pub shape: Vec<usize>,
    /// Flattened values.
    pub values: Vec<f32>,
}

/// An information-set-scoped board, without transport metadata.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Observation {
    /// Board with coordinates and a status line.
    pub text: String,
    /// Structured board conforming to the observation schema.
    pub json: Value,
    /// Optional numeric representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tensor: Option<Tensor>,
}

/// All supported representations of a legal move.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LegalAction {
    /// Canonical notation.
    pub string: String,
    /// Structured representation.
    pub json: Value,
    /// Index in the fixed action space.
    pub index: u32,
}

/// An engine event with explicit viewer scope.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GameEvent {
    /// Domain-specific event kind, such as chance.
    pub kind: String,
    /// Event contents.
    pub payload: Value,
    /// None means public; Some lists the seats that may see the payload.
    pub visible_to: Option<Vec<PlayerId>>,
}

/// Transition metadata; terminal returns are obtained from the engine.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StepEvents {
    /// Events caused by this action.
    pub events: Vec<GameEvent>,
    /// True for a time/length cap rather than a rules-based ending.
    pub truncated: bool,
}

/// Generate a JSON Schema from the authoritative Rust type.
pub fn schema<T: JsonSchema>() -> Value {
    serde_json::json!(schemars::schema_for!(T))
}

/// Engine-authored explanations that complement the machine-readable GameSpec.
///
/// Examples and starting boards must be generated by executing the engine; this
/// metadata supplies only the human explanations that cannot be inferred safely.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayGuide {
    /// Win, loss, and draw conditions.
    pub objective: String,
    /// Text orientation, coordinates, symbols, and JSON layout.
    pub observation: String,
    /// Tensor shape and plane semantics, or an explicit absence statement.
    pub tensor: String,
    /// When rewards occur and which shaping options exist.
    pub rewards: String,
    /// Effect of supported config fields, including an explicit no-options case.
    pub config: String,
    /// Known solved status, or an explicit unknown/not-applicable statement.
    pub solved: String,
    /// Frequent notation or rule mistakes.
    pub common_mistakes: Vec<String>,
    /// Optional neutral strategy hints; omitted from compact briefings.
    pub strategy_notes: Vec<String>,
}

impl PlayGuide {
    /// Check that required explanations are present before serving a briefing.
    pub fn validate(&self) -> Result<(), crate::GameError> {
        if [
            &self.objective,
            &self.observation,
            &self.tensor,
            &self.rewards,
            &self.config,
            &self.solved,
        ]
        .iter()
        .any(|text| text.trim().is_empty())
            || self.common_mistakes.is_empty()
            || self
                .common_mistakes
                .iter()
                .any(|text| text.trim().is_empty())
            || self
                .strategy_notes
                .iter()
                .any(|text| text.trim().is_empty())
        {
            return Err(crate::GameError::new(
                crate::ErrorCode::InvalidConfig,
                "Engine play guide is incomplete",
                "Supply every required explanation; state explicitly when a feature is absent.",
            ));
        }
        Ok(())
    }
}

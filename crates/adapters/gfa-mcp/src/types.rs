use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct Empty {}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct GameArgs {
    /// Registered game ID, for example chess.
    pub game_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct InfoArgs {
    /// Registered game ID, for example chess.
    pub game_id: String,
    /// Compact omits long examples and schemas; full includes them. Defaults to compact.
    #[serde(default)]
    pub detail: Detail,
    /// Options for a standalone game briefing. Omit when using match_id.
    #[serde(default)]
    pub config: Option<Value>,
    /// Optional existing match; the host supplies the authorized viewer.
    #[serde(default)]
    pub match_id: Option<String>,
}
#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub(crate) enum Detail {
    #[default]
    Compact,
    Full,
}
impl From<Detail> for gfa_api_types::InfoDetail {
    fn from(value: Detail) -> Self {
        match value {
            Detail::Compact => Self::Compact,
            Detail::Full => Self::Full,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct GameSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub players: [u8; 2],
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct GamesOutput {
    pub games: Vec<GameSummary>,
}

#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct Level {
    pub level: u8,
    pub rating: Option<f64>,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct OpponentSummary {
    pub id: String,
    pub name: String,
    pub levels: Vec<Level>,
    pub calibrated: bool,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct OpponentsOutput {
    pub opponents: Vec<OpponentSummary>,
}

#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct Section {
    pub id: String,
    pub text: String,
    pub data: Value,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct BriefingOutput {
    pub info_version: String,
    pub approx_tokens: usize,
    pub sections: Vec<Section>,
}
impl From<gfa_api_types::Briefing> for BriefingOutput {
    fn from(briefing: gfa_api_types::Briefing) -> Self {
        Self {
            info_version: briefing.info_version,
            approx_tokens: briefing.approx_tokens,
            sections: briefing
                .sections
                .into_iter()
                .map(|section| Section {
                    id: section.id,
                    text: section.text,
                    data: section.data,
                })
                .collect(),
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct Failure {
    pub code: String,
    pub message: String,
    pub hint: String,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(untagged)]
pub(crate) enum Output<T> {
    Success { data: T },
    Failure { error: Failure },
}

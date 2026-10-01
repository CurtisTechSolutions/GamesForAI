use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct MatchArgs {
    pub match_id: String,
}
#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateArgs {
    pub game_id: String,
    /// Installed opponent for the other seat; omit for externally controlled seats.
    pub opponent: Option<String>,
    /// Opponent level from list_opponents, 1 through 10 when supported.
    pub level: Option<u8>,
    /// Zero-based seat. Must agree with the viewer authorized by this host.
    pub play_as: Option<u8>,
    pub seed: Option<u64>,
    pub config: Option<Value>,
    /// Validated engine position notation; omit for the standard start.
    pub start: Option<String>,
    /// Match assistance policy. Analysis defaults to disabled.
    #[serde(default)]
    pub allow_analysis: bool,
    /// Simulation defaults to enabled.
    #[serde(default = "yes")]
    pub allow_simulation: bool,
}
fn yes() -> bool {
    true
}
#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct MoveArgs {
    pub match_id: String,
    /// Canonical string from get_legal_actions, game JSON, or {"index":n}.
    pub action: Value,
    pub reasoning: Option<String>,
    /// Optional compare-and-swap turn from get_state; prevents a stale move.
    pub turn: Option<u64>,
    /// Retry key, 1..128 bytes. Supply turn when using this key.
    pub idempotency_key: Option<String>,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct StateOutput {
    pub match_id: String,
    pub you: Option<u8>,
    pub turn: u64,
    pub to_act: Vec<u8>,
    /// Coordinate-labeled engine observation, with no tensor or redundant JSON board.
    pub board: String,
    pub legal_actions: Vec<String>,
    pub returns: Vec<f64>,
    pub terminated: bool,
    pub truncated: bool,
    pub outcome: Option<Value>,
    pub draw_offer: Option<u8>,
}
impl StateOutput {
    pub fn from_state(
        state: gfa_api_types::MatchState,
        viewer: gfa_core::Viewer,
    ) -> Result<Self, gfa_api_types::ApiError> {
        let mut legal_actions = state
            .legal_actions
            .into_iter()
            .map(|action| action.string)
            .collect::<Vec<_>>();
        legal_actions.sort();
        let outcome = state
            .outcome
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| crate::internal())?;
        Ok(Self {
            match_id: state.match_id,
            you: match viewer {
                gfa_core::Viewer::Player(seat) => Some(seat),
                _ => None,
            },
            turn: state.turn,
            to_act: state.to_act,
            board: state.observation.text,
            legal_actions,
            returns: state.returns,
            terminated: state.terminated,
            truncated: state.truncated,
            outcome,
            draw_offer: state.draw_offer,
        })
    }
    pub fn text(&self) -> String {
        format!("Match: {}\nTurn: {}. To act: {:?}. Your seat: {:?}.\n{}\nLegal actions: {}\nTerminated: {}. Truncated: {}. Returns: {:?}. Outcome: {}",
            self.match_id,self.turn,self.to_act,self.you,self.board,self.legal_actions.join(", "),
            self.terminated,self.truncated,self.returns,self.outcome.as_ref().map_or_else(||"none".into(),Value::to_string))
    }
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct CreatedOutput {
    pub state: StateOutput,
    pub info: crate::types::BriefingOutput,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct LegalOutput {
    pub match_id: String,
    pub turn: u64,
    pub to_act: Vec<u8>,
    pub legal_actions: Vec<String>,
    pub notation: String,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct Reply {
    pub seat: u8,
    pub turn: u64,
    /// Only actions disclosed by the service's viewer projection are included.
    pub action: Option<String>,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct MovedOutput {
    pub accepted_action: String,
    pub opponent_actions: Vec<Reply>,
    pub state: StateOutput,
}

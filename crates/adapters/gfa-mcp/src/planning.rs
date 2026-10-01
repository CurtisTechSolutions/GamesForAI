use crate::{decode, internal, play_types::StateOutput, success, types::Failure, McpServer};
use gfa_api_types::{
    AnalysisRequest, ApiError, OpponentConfig, SearchBudget, SimulateRequest, SimulationFrom,
    SimulationOutput, UciOptions,
};
use rmcp::{model::CallToolResult, schemars::JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct SimulationArgs {
    pub match_id: String,
    /// Up to 16 independent lines, each with at most 32 canonical moves.
    pub lines: Vec<Vec<Value>>,
    pub from_turn: Option<u64>,
    /// Independent planning seed; never changes the match RNG.
    pub seed: Option<u64>,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct LineError {
    pub index: usize,
    pub error: Failure,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct Line {
    pub moves_applied: u32,
    pub state: StateOutput,
    pub error: Option<LineError>,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct Simulation {
    pub game_id: String,
    pub seed: u64,
    pub lines: Vec<Line>,
}
#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisArgs {
    pub match_id: String,
    pub turn: Option<u64>,
    /// Installed analysis provider; defaults to Stockfish, reference, minimax, MCTS, then random.
    pub opponent: Option<String>,
    pub level: Option<u8>,
    pub nodes: Option<u64>,
    pub depth: Option<u8>,
    pub time_ms: Option<u64>,
    /// 1..16 ranked lines; only supported by UCI providers.
    pub multi_pv: Option<u8>,
    pub seed: Option<u64>,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct Recommendation {
    pub action: String,
    /// Expected game return, never a centipawn or mate score.
    pub evaluation: Option<f64>,
    pub principal_variation: Vec<String>,
    pub nodes: u64,
    pub depth: u8,
    pub budget_exhausted: bool,
    /// Provider explanation and labeled score units, if available.
    pub advice: Option<Value>,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct Analysis {
    pub game_id: String,
    pub opponent: String,
    pub seed: u64,
    pub variations: Vec<Recommendation>,
}

impl McpServer {
    pub(crate) async fn simulate_tool(&self, arguments: Value) -> Result<CallToolResult, ApiError> {
        let args: SimulationArgs = decode(arguments)?;
        let seat = self.player()?;
        let game = self.service.get_match(&args.match_id).await?.game_id;
        let result = self
            .service
            .simulate(
                &game,
                SimulateRequest {
                    from: SimulationFrom::Match {
                        match_id: args.match_id,
                        seat,
                        turn: args.from_turn,
                    },
                    config: json!({}),
                    lines: args.lines,
                    seed: args.seed,
                    output: SimulationOutput::Final,
                },
                self.viewer,
            )
            .await?;
        let lines = result
            .lines
            .into_iter()
            .map(|line| {
                let state = StateOutput::from_state(
                    line.states.into_iter().last().ok_or_else(internal)?,
                    self.viewer,
                )?;
                Ok(Line {
                    moves_applied: line.moves_applied,
                    state,
                    error: line.error.map(|error| LineError {
                        index: error.index,
                        error: crate::compact_failure(error.error),
                    }),
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        let text = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let error = line
                    .error
                    .as_ref()
                    .map(|failure| {
                        format!(
                            "\nStopped at move {}: {} — {}",
                            failure.index, failure.error.message, failure.error.hint
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "Line {}: {} moves applied{}\n{}",
                    index + 1,
                    line.moves_applied,
                    error,
                    line.state.text()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        success(
            Simulation {
                game_id: game,
                seed: result.seed,
                lines,
            },
            text,
        )
    }
    pub(crate) async fn analyze_tool(&self, arguments: Value) -> Result<CallToolResult, ApiError> {
        let args: AnalysisArgs = decode(arguments)?;
        let seat = self.player()?;
        let game = self.service.get_match(&args.match_id).await?.game_id;
        let opponent = if let Some(opponent) = args.opponent {
            opponent
        } else {
            let catalog = self.service.list_opponents(&game)?;
            ["stockfish", "reference", "minimax", "mcts", "random"]
                .into_iter()
                .find(|id| catalog.iter().any(|candidate| candidate.id == *id))
                .ok_or_else(|| {
                    ApiError::new(
                        "ENGINE_UNAVAILABLE",
                        "No analysis provider is installed",
                        "Install a compatible opponent.",
                    )
                })?
                .to_string()
        };
        let result = self
            .service
            .analyze(
                AnalysisRequest {
                    game_id: game.clone(),
                    from: SimulationFrom::Match {
                        match_id: args.match_id,
                        seat,
                        turn: args.turn,
                    },
                    config: json!({}),
                    opponent: OpponentConfig {
                        id: opponent,
                        level: args.level,
                        limits: SearchBudget {
                            nodes: args.nodes,
                            depth: args.depth,
                            time_ms: args.time_ms,
                        },
                        uci: args.multi_pv.map(|multipv| UciOptions {
                            multipv: Some(multipv),
                            ..Default::default()
                        }),
                    },
                    seed: args.seed,
                },
                self.viewer,
            )
            .await?;
        let variations = result
            .variations
            .into_iter()
            .map(|choice| {
                Ok(Recommendation {
                    action: choice.action.string,
                    evaluation: choice.info.evaluation,
                    principal_variation: choice.info.principal_variation,
                    nodes: choice.info.nodes,
                    depth: choice.info.depth,
                    budget_exhausted: choice.info.budget_exhausted,
                    advice: choice
                        .info
                        .advice
                        .map(serde_json::to_value)
                        .transpose()
                        .map_err(|_| internal())?,
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        let text = variations.iter().enumerate().map(|(index,line)|format!(
            "{}. {} — expected return: {:?}, depth: {}, nodes: {}, budget exhausted: {}\nPV: {}\nAdvice: {}",
            index+1,line.action,line.evaluation,line.depth,line.nodes,line.budget_exhausted,line.principal_variation.join(" "),
            line.advice.as_ref().map_or_else(||"none".into(),Value::to_string)
        )).collect::<Vec<_>>().join("\n");
        success(
            Analysis {
                game_id: game,
                opponent: result.opponent,
                seed: result.seed,
                variations,
            },
            text,
        )
    }
}

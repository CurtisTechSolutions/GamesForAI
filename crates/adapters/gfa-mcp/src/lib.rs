//! Official-SDK MCP adapter over the transport-independent lifecycle service.
mod types;
use gfa_api_types::ApiError;
use gfa_core::Viewer;
use gfa_service::GameService;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    schemars::{self, JsonSchema},
    service::RequestContext,
    ErrorData, RoleServer, ServerHandler,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use types::*;

/// A handler whose viewer authorization is selected by the trusted host.
/// It never accepts an omniscient viewer or lets tool parameters elevate access.
#[derive(Clone)]
pub struct McpServer {
    service: Arc<GameService>,
    viewer: Viewer,
    tools: Arc<Vec<Tool>>,
}
impl McpServer {
    /// Build the game-discovery tools. Hosts must authorize the supplied viewer first.
    pub fn new(service: Arc<GameService>, viewer: Viewer) -> Result<Self, ApiError> {
        if viewer == Viewer::Omniscient {
            return Err(ApiError::new(
                "FORBIDDEN",
                "MCP does not expose omniscient access",
                "Select an authorized player or spectator view.",
            ));
        }
        Ok(Self { service, viewer, tools: Arc::new(vec![
            tool::<Empty, GamesOutput>("list_games", "Discover installed games before choosing one. Example: list_games({}).")?,
            tool::<InfoArgs, BriefingOutput>("get_game_info", "Read rules and the API loop before playing; use match_id for an existing match. Example: get_game_info({\"game_id\":\"chess\",\"detail\":\"compact\"}).")?,
            tool::<GameArgs, OpponentsOutput>("list_opponents", "Find installed opponents and their measured levels before creating a match. Example: list_opponents({\"game_id\":\"chess\"}).")?,
        ]) })
    }

    /// Tool definitions with input/output schemas and usage examples.
    pub fn tool_definitions(&self) -> Vec<Tool> {
        self.tools.as_ref().clone()
    }

    /// Dispatch a tool through the same service used by REST.
    /// Recoverable domain/input failures are readable tool errors, not JSON-RPC failures.
    pub async fn invoke(&self, name: &str, arguments: Value) -> Result<CallToolResult, ErrorData> {
        if !self.tools.iter().any(|tool| tool.name == name) {
            return Err(ErrorData::new(
                rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                "Unknown tool",
                None,
            ));
        }
        if serde_json::to_vec(&arguments).map_or(true, |bytes| bytes.len() > 64 * 1024) {
            return Ok(failure(ApiError::new(
                "INVALID_REQUEST",
                "Tool arguments exceed 64 KiB",
                "Send a smaller request.",
            )));
        }
        let result = match name {
            "list_games" => self.games(arguments),
            "list_opponents" => self.opponents(arguments),
            "get_game_info" => self.info(arguments).await,
            _ => {
                return Err(ErrorData::new(
                    rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                    "Unknown tool",
                    None,
                ))
            }
        };
        Ok(result.unwrap_or_else(failure))
    }
    fn games(&self, arguments: Value) -> Result<CallToolResult, ApiError> {
        let _: Empty = decode(arguments)?;
        let games = self
            .service
            .list_games()
            .into_iter()
            .map(|game| GameSummary {
                id: game.id,
                name: game.name,
                description: game.summary,
                players: game.num_players,
            })
            .collect::<Vec<_>>();
        let text = games
            .iter()
            .map(|game| format!("{} — {}: {}", game.id, game.name, game.description))
            .collect::<Vec<_>>()
            .join("\n");
        success(GamesOutput { games }, text)
    }
    fn opponents(&self, arguments: Value) -> Result<CallToolResult, ApiError> {
        let args: GameArgs = decode(arguments)?;
        let opponents = self
            .service
            .list_opponents(&args.game_id)?
            .into_iter()
            .map(|opponent| OpponentSummary {
                id: opponent.id,
                name: opponent.name,
                calibrated: opponent.calibrated,
                levels: opponent
                    .levels
                    .into_iter()
                    .map(|level| Level {
                        level: level.level,
                        rating: level.rating,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let text = if opponents.is_empty() {
            "No opponents are installed for this game.".into()
        } else {
            opponents
                .iter()
                .map(|opponent| {
                    format!(
                        "{} — {} ({} levels, calibrated: {})",
                        opponent.id,
                        opponent.name,
                        opponent.levels.len(),
                        opponent.calibrated
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        success(OpponentsOutput { opponents }, text)
    }
    async fn info(&self, arguments: Value) -> Result<CallToolResult, ApiError> {
        let args: InfoArgs = decode(arguments)?;
        let briefing = if let Some(id) = args.match_id {
            if args.config.is_some() {
                return Err(ApiError::new(
                    "INVALID_CONFIG",
                    "An existing match already has its configuration",
                    "Omit config when supplying match_id.",
                ));
            }
            let metadata = self.service.get_match(&id).await?;
            if metadata.game_id != args.game_id {
                return Err(ApiError::new(
                    "INVALID_CONFIG",
                    "The match belongs to another game",
                    "Use the game_id recorded for this match.",
                ));
            }
            self.service
                .get_match_info(&id, self.viewer, args.detail.into())
                .await?
        } else {
            let seat = match self.viewer {
                Viewer::Player(seat) => Some(seat),
                _ => None,
            };
            self.service.get_game_info(
                &args.game_id,
                &args.config.unwrap_or(json!({})),
                seat,
                args.detail.into(),
            )?
        };
        let text = briefing.markdown().map_err(|_| internal())?;
        success(BriefingOutput::from(briefing), text)
    }
}
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("GamesForAI", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Read get_game_info before playing. Tool errors include recovery hints.",
            )
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|tool| tool.name == name).cloned()
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|request| request.cursor.is_some()) {
            return Err(ErrorData::invalid_params(
                "This tool catalog has no further page",
                None,
            ));
        }
        Ok(ListToolsResult::with_all_items(self.tool_definitions()))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.invoke(
            &request.name,
            Value::Object(request.arguments.unwrap_or_default()),
        )
        .await
        .map(Into::into)
    }
}
fn tool<I: JsonSchema, O: JsonSchema>(
    name: &'static str,
    description: &'static str,
) -> Result<Tool, ApiError> {
    let input = serde_json::to_value(schemars::schema_for!(I))
        .map_err(|_| internal())?
        .as_object()
        .cloned()
        .ok_or_else(internal)?;
    let output = serde_json::to_value(schemars::schema_for!(Output<O>))
        .map_err(|_| internal())?
        .as_object()
        .cloned()
        .ok_or_else(internal)?;
    Ok(Tool::new(name, description, input).with_raw_output_schema(Arc::new(output)))
}
fn decode<T: DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    serde_json::from_value(value).map_err(|error| {
        ApiError::new(
            "INVALID_REQUEST",
            error.to_string(),
            "Follow this tool's input schema and example call.",
        )
    })
}
fn success<T: Serialize>(data: T, text: String) -> Result<CallToolResult, ApiError> {
    let value = serde_json::to_value(Output::Success { data }).map_err(|_| internal())?;
    if text.len() + serde_json::to_vec(&value).map_err(|_| internal())?.len() > 1024 * 1024 {
        return Err(ApiError::new(
            "RESPONSE_TOO_LARGE",
            "Tool result exceeds 1 MiB",
            "Request compact information.",
        ));
    }
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = Some(value);
    Ok(result)
}
fn failure(error: ApiError) -> CallToolResult {
    let mut result = CallToolResult::error(vec![ContentBlock::text(format!(
        "{}: {}\n{}",
        error.code, error.message, error.hint
    ))]);
    result.structured_content = Some(json!(Output::<Value>::Failure {
        error: Failure {
            code: error.code,
            message: error.message,
            hint: error.hint,
        }
    }));
    result
}
fn internal() -> ApiError {
    ApiError::new(
        "INVALID_MCP_RESPONSE",
        "MCP response could not be constructed",
        "Report the adapter failure.",
    )
}

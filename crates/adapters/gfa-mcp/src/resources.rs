use crate::{decode, play_types::StateOutput, types::GameArgs, McpServer};
use gfa_api_types::{ApiError, InfoDetail};
use gfa_core::Viewer;
use rmcp::{model::{GetPromptResult, ListPromptsResult, ListResourcesResult, ListResourceTemplatesResult, PaginatedRequestParams, Prompt, PromptArgument, PromptMessage, ReadResourceResult, Resource, ResourceContents, ResourceTemplate, Role}, ErrorData};
use serde_json::{json, Value};

pub(crate) fn no_cursor(request: Option<PaginatedRequestParams>) -> Result<(),ErrorData> {
    if request.is_some_and(|request|request.cursor.is_some()) {
        return Err(ErrorData::invalid_params("This catalog has no further page",None));
    }
    Ok(())
}
fn protocol(error: ApiError) -> ErrorData {
    ErrorData::invalid_params(format!("{}: {} — {}",error.code,error.message,error.hint),Some(json!({"code":error.code,"hint":error.hint})))
}
impl McpServer {
    pub(crate) fn resources(&self) -> ListResourcesResult {
        ListResourcesResult::with_all_items(self.service.list_games().into_iter().map(|game|
            Resource::new(format!("gfa://games/{}/info",game.id),format!("{} rules",game.name))
                .with_description("Compact game briefing").with_mime_type("text/markdown")
        ).collect())
    }
    pub(crate) fn templates(&self) -> ListResourceTemplatesResult {
        ListResourceTemplatesResult::with_all_items([
            ("gfa://games/{game_id}/info","Game rules","text/markdown"),
            ("gfa://matches/{match_id}/info","Match briefing","text/markdown"),
            ("gfa://matches/{match_id}/state","Live match state","application/json"),
            ("gfa://matches/{match_id}/replay","Recorded moves","application/json"),
        ].into_iter().map(|(uri,name,mime)|ResourceTemplate::new(uri,name).with_mime_type(mime)).collect())
    }
    pub(crate) fn prompts(&self) -> ListPromptsResult {
        ListPromptsResult::with_all_items(vec![Prompt::new(
            "play_game",Some("Read the compact rules and follow the MCP game loop."),
            Some(vec![PromptArgument::new("game_id").with_required(true).with_description("Registered game ID from list_games")]),
        )])
    }
    /// Read a bounded resource using exactly this adapter's host-authorized viewer.
    pub async fn resource(&self, uri: &str) -> Result<ReadResourceResult,ErrorData> {
        let suffix = uri.strip_prefix("gfa://").ok_or_else(||ErrorData::invalid_params("Expected a gfa:// resource URI",None))?;
        if uri.len() > 512 || suffix.bytes().any(|byte|!byte.is_ascii_alphanumeric() && !b"/_-".contains(&byte)) {
            return Err(ErrorData::invalid_params("Resource URI contains invalid characters",None));
        }
        let parts = suffix.split('/').collect::<Vec<_>>();
        let (text,mime) = match parts.as_slice() {
            ["games",id,"info"] => {
                let seat=match self.viewer {Viewer::Player(seat)=>Some(seat),_=>None};
                let info=self.service.get_game_info(id,&json!({}),seat,InfoDetail::Compact).map_err(protocol)?;
                (info.markdown().map_err(|_|protocol(crate::internal()))?,"text/markdown")
            }
            ["matches",id,"info"] => {
                let info=self.service.get_match_info(id,self.viewer,InfoDetail::Compact).await.map_err(protocol)?;
                (info.markdown().map_err(|_|protocol(crate::internal()))?,"text/markdown")
            }
            ["matches",id,"state"] => {
                let state=self.service.get_state(id,self.viewer).await.map_err(protocol)?;
                let output=StateOutput::from_state(state,self.viewer).map_err(protocol)?;
                (serde_json::to_string(&output).map_err(|_|protocol(crate::internal()))?,"application/json")
            }
            ["matches",id,"replay"] => {
                let output=self.replay_tool(json!({"match_id":id,"limit":100})).await.map_err(protocol)?;
                let data=output.structured_content.and_then(|value|value.get("data").cloned()).ok_or_else(||protocol(crate::internal()))?;
                (serde_json::to_string(&data).map_err(|_|protocol(crate::internal()))?,"application/json")
            }
            _=>return Err(ErrorData::invalid_params("Unknown GamesForAI resource",None)),
        };
        if text.len() > 1024*1024 {
            return Err(ErrorData::invalid_params("Resource exceeds 1 MiB; use compact paginated tools",None));
        }
        let mut result = ReadResourceResult::new(vec![ResourceContents::text(text,uri).with_mime_type(mime)]);
        result.ttl_ms = Some(0);
        result.cache_scope = Some(rmcp::model::CacheScope::Private);
        Ok(result)
    }
    /// Build a user-invoked play prompt. MCP prompt messages have user/assistant roles.
    pub fn play_prompt(&self, name: &str, arguments: Value) -> Result<GetPromptResult,ErrorData> {
        if name != "play_game" { return Err(ErrorData::invalid_params("Unknown prompt",None)); }
        if serde_json::to_vec(&arguments).map_or(true,|value|value.len()>8192) {
            return Err(ErrorData::invalid_params("Prompt arguments exceed 8 KiB",None));
        }
        let args:GameArgs=decode(arguments).map_err(protocol)?;
        let seat=match self.viewer {Viewer::Player(seat)=>Some(seat),_=>None};
        let info=self.service.get_game_info(&args.game_id,&json!({}),seat,InfoDetail::Compact).map_err(protocol)?;
        let text=format!("Play {} through GamesForAI tools. Read the rules below, then list_opponents and create_match. Choose only listed legal actions. Each make_move response includes automatic opponent replies and the new state. If a move fails, use its hint and legal actions to retry. Optional reasoning is recorded. Use simulation or analysis only when the match permits it. Stop when terminated or truncated is true; report the recorded outcome.\n\n{}",args.game_id,info.markdown().map_err(|_|protocol(crate::internal()))?);
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(Role::User,text)]).with_description(format!("Play {}",args.game_id)))
    }
}

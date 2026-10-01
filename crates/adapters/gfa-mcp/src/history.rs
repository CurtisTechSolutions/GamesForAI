use crate::{decode, internal, success, McpServer};
use gfa_api_types::{ApiError, EventData, EventsQuery, MatchHistoryQuery, MatchStatus};
use rmcp::{model::CallToolResult, schemars::JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

fn page_size() -> u32 {
    25
}
#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct HistoryArgs {
    pub game_id: Option<String>,
    /// Maximum records scanned per page, 1..100; defaults to 25.
    #[serde(default = "page_size")]
    pub limit: u32,
    /// Cursor returned by the previous page, including empty filtered pages.
    pub after: Option<String>,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct MatchSummary {
    pub match_id: String,
    pub game_id: String,
    pub status: String,
    pub turn: u64,
    pub returns: Vec<f64>,
    pub created_at_ms: u64,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct HistoryOutput {
    pub matches: Vec<MatchSummary>,
    pub next: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayArgs {
    pub match_id: String,
    /// Exclusive sequence from the preceding replay page.
    pub since: Option<u64>,
    /// Maximum events per page, 1..100; defaults to 25.
    #[serde(default = "page_size")]
    pub limit: u32,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct ReplayEntry {
    pub sequence: u64,
    pub kind: String,
    pub turn: Option<u64>,
    pub seat: Option<u8>,
    pub action: Option<String>,
    pub reasoning: Option<String>,
    /// Viewer-scoped annotations and public metadata; never a raw storage event.
    pub details: Value,
}
#[derive(Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub(crate) struct ReplayOutput {
    pub match_id: String,
    pub revision: u64,
    pub events: Vec<ReplayEntry>,
    pub next: Option<u64>,
}
impl McpServer {
    pub(crate) async fn history_tool(&self, arguments: Value) -> Result<CallToolResult, ApiError> {
        let args: HistoryArgs = decode(arguments)?;
        let page = self
            .service
            .list_matches(MatchHistoryQuery {
                game_id: args.game_id,
                limit: args.limit,
                after: args.after,
                status: None,
            })
            .await?;
        let matches = page
            .matches
            .into_iter()
            .map(|entry| MatchSummary {
                match_id: entry.match_id,
                game_id: entry.game_id,
                status: match entry.status {
                    MatchStatus::Active => "active",
                    MatchStatus::Finished => "finished",
                }
                .into(),
                turn: entry.turn,
                returns: entry.returns,
                created_at_ms: entry.created_at_ms,
            })
            .collect::<Vec<_>>();
        let text = format!(
            "{}\nNext cursor: {}",
            matches
                .iter()
                .map(|entry| format!(
                    "{}: {} — {}, {} moves, returns {:?}",
                    entry.match_id, entry.game_id, entry.status, entry.turn, entry.returns
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            page.next.as_deref().unwrap_or("none")
        );
        success(
            HistoryOutput {
                matches,
                next: page.next,
            },
            text,
        )
    }
    pub(crate) async fn replay_tool(&self, arguments: Value) -> Result<CallToolResult, ApiError> {
        let args: ReplayArgs = decode(arguments)?;
        let page = self
            .service
            .get_events(
                &args.match_id,
                EventsQuery {
                    since: args.since,
                    limit: args.limit,
                    seat: None,
                },
                self.viewer,
            )
            .await?;
        let mut events = Vec::with_capacity(page.events.len());
        for entry in page.events {
            let mut output = ReplayEntry {
                sequence: entry.sequence,
                kind: String::new(),
                turn: None,
                seat: None,
                action: None,
                reasoning: None,
                details: Value::Null,
            };
            match entry.event {
                EventData::Created {
                    game_id,
                    engine_version,
                    config,
                    assists,
                    at_ms,
                    ..
                } => {
                    output.kind = "created".into();
                    output.details = json!({"game_id":game_id,"engine_version":engine_version,"config":config,"assists":assists,"at_ms":at_ms});
                }
                EventData::Action {
                    turn,
                    seat,
                    action,
                    reasoning,
                    opponent_info,
                    events,
                    at_ms,
                } => {
                    output.kind = "action".into();
                    output.turn = Some(turn);
                    output.seat = Some(seat);
                    output.action = action.map(|action| action.string);
                    output.reasoning = reasoning;
                    output.details =
                        json!({"opponent_info":opponent_info,"events":events,"at_ms":at_ms});
                }
                EventData::ForkedFrom { source } => {
                    output.kind = "forked_from".into();
                    output.details = serde_json::to_value(source).map_err(|_| internal())?;
                }
                EventData::Resigned { turn, seat, at_ms } => {
                    output.kind = "resigned".into();
                    output.turn = Some(turn);
                    output.seat = Some(seat);
                    output.details = json!({"at_ms":at_ms});
                }
                EventData::DrawOffered { turn, seat, at_ms } => {
                    output.kind = "draw_offered".into();
                    output.turn = Some(turn);
                    output.seat = Some(seat);
                    output.details = json!({"at_ms":at_ms});
                }
                EventData::Finished {
                    turn,
                    returns,
                    terminated,
                    truncated,
                } => {
                    output.kind = "finished".into();
                    output.turn = Some(turn);
                    output.details =
                        json!({"returns":returns,"terminated":terminated,"truncated":truncated});
                }
            }
            events.push(output);
        }
        let text = format!(
            "Match: {}. Revision: {}. Next sequence: {:?}.\n{}",
            args.match_id,
            page.revision,
            page.next,
            events
                .iter()
                .map(|entry| format!(
                    "#{} {} turn={:?} seat={:?} action={} reasoning={} details={}",
                    entry.sequence,
                    entry.kind,
                    entry.turn,
                    entry.seat,
                    entry.action.as_deref().unwrap_or("none/private"),
                    entry.reasoning.as_deref().unwrap_or("none/private"),
                    entry.details
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        success(
            ReplayOutput {
                match_id: args.match_id,
                revision: page.revision,
                events,
                next: page.next,
            },
            text,
        )
    }
}

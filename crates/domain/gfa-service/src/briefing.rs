use crate::{error, replay::Frame};
use gfa_api_types::{ApiError, Briefing, InfoDetail, InfoSection};
use gfa_core::{DynGame, Viewer};
use serde_json::{json, Value};

pub(crate) fn internal() -> ApiError {
    ApiError::new(
        "INVALID_BRIEFING",
        "Engine briefing cannot be generated",
        "Check the engine's play guide and conformance results.",
    )
}

fn section(id: &str, text: impl Into<String>, data: Value) -> InfoSection {
    InfoSection {
        id: id.into(),
        text: text.into(),
        data,
    }
}

fn preview(
    frame: &Frame,
    game: &dyn DynGame,
    viewer: Viewer,
    full: bool,
) -> Result<Value, ApiError> {
    let mut value =
        serde_json::to_value(frame.project("example", game, viewer)?).map_err(|_| internal())?;
    if let Some(object) = value.as_object_mut() {
        object.remove("match_id");
        if !full {
            object.remove("action_mask");
            if let Some(observation) = object.get_mut("observation").and_then(Value::as_object_mut)
            {
                observation.remove("tensor");
            }
        }
    }
    Ok(value)
}

/// Build a synthetic briefing without touching a match, clock, ID source, or store.
pub(crate) fn game_info(
    game: &dyn DynGame,
    config: &Value,
    seat: Option<u8>,
    detail: InfoDetail,
) -> Result<Briefing, ApiError> {
    game_info_with_viewer(game, config, seat, detail, None)
}

fn game_info_with_viewer(
    game: &dyn DynGame,
    config: &Value,
    seat: Option<u8>,
    detail: InfoDetail,
    sample_viewer: Option<Viewer>,
) -> Result<Briefing, ApiError> {
    let spec = game.spec();
    let guide = game.play_guide().ok_or_else(internal)?;
    guide.validate().map_err(|_| internal())?;
    let config = game.normalize_config(config).map_err(error::engine)?;
    let state = game.initial_state(&config, 0).map_err(error::engine)?;
    let actors = game.current_players(&state).map_err(error::engine)?;
    let first = actors.first().copied().ok_or_else(internal)?;
    let viewer = sample_viewer.unwrap_or(Viewer::Player(seat.unwrap_or(first)));
    let initial = Frame {
        state,
        turn: 0,
        terminated: false,
        truncated: false,
    };
    let full = detail == InfoDetail::Full;
    let initial_view = preview(&initial, game, viewer, full)?;
    let actions = game
        .legal_actions(&initial.state, first)
        .map_err(error::engine)?;
    let you = seat.map(|seat| {
        json!({
            "seat":seat, "name":spec.seat_names.get(usize::from(seat)),
            "moves_first":actors.contains(&seat)
        })
    });
    let mut examples = Vec::new();
    let mut frame = Frame {
        state: initial.state.clone(),
        turn: 0,
        terminated: false,
        truncated: false,
    };
    for _ in 0..if full { 2 } else { 1 } {
        let player = game
            .current_players(&frame.state)
            .map_err(error::engine)?
            .first()
            .copied()
            .ok_or_else(internal)?;
        let legal = game
            .legal_actions(&frame.state, player)
            .map_err(error::engine)?;
        let action = legal.first().ok_or_else(internal)?;
        let request = json!({"seat":player,"turn":frame.turn,"action":action.string});
        let (state, events) = game
            .apply(&frame.state, player, &json!(action.string))
            .map_err(error::engine)?;
        frame = crate::replay::advance(game, state, frame.turn + 1, &events)?;
        examples.push(json!({
            "note":"Accepted in this synthetic example; use your live match's current turn.",
            "request":request,
            "response":if full { preview(&frame, game, viewer, true)? }
                else { json!({"turn":frame.turn,"to_act":game.current_players(&frame.state).map_err(error::engine)?}) }
        }));
        if frame.ended() {
            break;
        }
    }
    let invalid = json!({"index":spec.action_space_size});
    let failure = game
        .apply(&initial.state, first, &invalid)
        .err()
        .ok_or_else(internal)?;
    examples.push(json!({
        "note":"Rejected at the initial state; the state is unchanged.",
        "request":{"seat":first,"turn":0,"action":invalid},
        "error":error::engine(failure)
    }));
    let mut action_data = json!({
        "action_space_size":spec.action_space_size,
        "examples":actions.iter().take(if full { 3 } else { 1 }).collect::<Vec<_>>(),
        "common_mistakes":guide.common_mistakes
    });
    let mut observation_data = json!({
        "tensor_shape":game.observe(&initial.state, viewer).map_err(error::engine)?.tensor.map(|tensor|tensor.shape)
    });
    let mut config_data = json!({
        "active":config,
        "defaults":game.normalize_config(&Value::Null).map_err(error::engine)?
    });
    if full {
        action_data["schema"] = spec.action_schema;
        observation_data["schema"] = spec.observation_schema;
        config_data["schema"] = spec.config_schema;
    }
    let mut result = Briefing {
        info_version: format!("{}+briefing.1", spec.info_version),
        approx_tokens: 0,
        sections: vec![
            section("identity", spec.summary, json!({
                "game_id":spec.id,"name":spec.name,"engine_version":spec.engine_version,
                "info_version":spec.info_version
            })),
            section("players", "Seat IDs are zero-based. Follow to_act on each response.", json!({
                "count":spec.num_players,
                "seats":spec.seat_names.iter().enumerate().map(|(seat,name)|json!({"seat":seat,"name":name})).collect::<Vec<_>>(),
                "first_to_act":actors,"turn_structure":spec.turn_structure,"you":you
            })),
            section("properties", guide.solved, json!({
                "information":spec.information,"stochastic":spec.stochastic,
                "simultaneous":spec.turn_structure == gfa_core::TurnStructure::Simultaneous,
                "max_game_length":spec.max_game_length
            })),
            section("objective", guide.objective, Value::Null),
            section("rules", spec.rules_markdown, Value::Null),
            section("action_format", spec.action_notation, action_data),
            section("observation_format", format!("{} {}", guide.observation, guide.tensor), observation_data),
            section("initial_state", "Synthetic standard start, seed 0; independent of every live match.", initial_view),
            section("example_turns", "Requests and results generated by executing this engine.", json!(examples)),
            section("rewards", guide.rewards, json!({"range":spec.reward_range})),
            section("config_options", guide.config, config_data),
            section("time_controls", "Untimed play only; no running clocks or timeout penalties.", json!({"supported":["none"],"default":"none"})),
            section("opponents", "Local clients control the seats. Built-in opponents, analysis, and simulation are not installed.", json!({"available":[],"analysis":false,"simulation":false})),
            section("illegal_move_policy", "Reject without changing state; retry using the returned hint and current legal actions.", json!({"supported":["reject"],"default":"reject"})),
            section("how_to_play", "Create a match, then submit moves for seats in to_act until terminated or truncated. MCP is not available yet.", json!({
                "create":{"method":"POST","path":"/v1/matches","body":{"game_id":spec.id,"config":config}},
                "state":"GET /v1/matches/{id}/state?seat={seat}",
                "move":"POST /v1/matches/{id}/actions with {seat,turn,action,reasoning?}",
                "retry":"Send the same Idempotency-Key and identical request to recover its original result.",
                "replay":"GET /v1/matches/{id}/replay?seat={seat}"
            })),
            section("strategy_notes", if !full { "Omitted in compact mode." } else if guide.strategy_notes.is_empty() { "No strategy notes supplied." } else { "Optional basic principles." }, if full { json!(guide.strategy_notes) } else { Value::Null }),
            section("limits", "No per-agent rate limiter or guaranteed observation token ceiling in local mode.", json!({"max_game_length":spec.max_game_length})),
        ],
    };
    result.estimate_tokens().map_err(|_| internal())?;
    Ok(result)
}

pub(crate) struct MatchBrief<'a> {
    pub game: &'a dyn DynGame,
    pub origin: &'a crate::MatchOrigin,
    pub initial: &'a Frame,
    pub current: &'a Frame,
    pub id: &'a str,
    pub viewer: Viewer,
}

impl MatchBrief<'_> {
    pub fn build(self, detail: InfoDetail) -> Result<Briefing, ApiError> {
        let spec = self.game.spec();
        let seat = match self.viewer {
            Viewer::Player(seat) => Some(seat),
            _ => None,
        };
        // Validate the live projection first, including the requested seat.
        let current = self.current.project(self.id, self.game, self.viewer)?;
        let initial = self.initial.project(self.id, self.game, self.viewer)?;
        let mut info = game_info_with_viewer(
            self.game,
            &self.origin.config,
            seat,
            detail,
            Some(self.viewer),
        )?;
        let you =
            seat.map(|seat| json!({"seat":seat,"name":spec.seat_names.get(usize::from(seat))}));
        let start = if self.origin.start.is_none() {
            json!({"type":"standard"})
        } else if self.viewer == Viewer::Omniscient
            || (spec.information == gfa_core::Information::Perfect && !spec.stochastic)
        {
            json!({
                "type":"custom",
                "position":self.game.state_to_notation(&self.initial.state).map_err(error::engine)?
            })
        } else {
            // State notation may contain hidden cards or an RNG state. Never
            // expose it as match metadata merely because a caller can view play.
            json!({"type":"custom","view":initial.observation,
                "note":"Only this viewer's starting observation is available."})
        };
        info.sections.insert(0, section(
            "match",
            "Live match settings and viewer-scoped state. Later game examples use an independent synthetic start. Local clients control all seats; invalid moves can be retried without a limit.",
            json!({
                "match_id":self.id,
                "status":if self.current.ended() { "finished" } else { "active" },
                "turn":current.turn,"to_act":current.to_act,"you":you,
                "participants":spec.seat_names.iter().enumerate().map(|(seat,name)|
                    json!({"seat":seat,"name":name,"type":"external","level":null,"rating":null})).collect::<Vec<_>>(),
                "start":start,
                "time_control":{"type":"none","clocks":null},
                "illegal_move_policy":{"policy":"reject","attempts_remaining":null},
                "assists":{"allow_analysis":false,"allow_simulation":false},
                "task":{"type":"none","note":"No benchmark task is assigned."},
                "recording":{"moves":true,"reasoning":true,"llm_transcripts":false,
                    "visibility":"local","note":"Replays apply the requested viewer; transcripts are unavailable."},
                "state":current
            }),
        ));
        info.estimate_tokens().map_err(|_| internal())?;
        Ok(info)
    }
}

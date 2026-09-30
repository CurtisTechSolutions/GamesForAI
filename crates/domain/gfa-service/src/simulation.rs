use crate::{error, replay};
use gfa_api_types::{
    ApiError, SimulateRequest, SimulatedLine, SimulationFailure, SimulationOutput, SimulationResult,
};
use gfa_core::{DynGame, Observation, Viewer};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) fn fingerprint(config: &Value, observation: &Observation) -> Result<[u8; 32], ApiError> {
    let bytes = serde_json::to_vec(&(config, &observation.json))
        .map_err(|error| error::engine(error.into()))?;
    Ok(Sha256::digest(bytes).into())
}

pub(crate) fn validate(request: &SimulateRequest) -> Result<(), ApiError> {
    if request.lines.is_empty()
        || request.lines.len() > 16
        || request.lines.iter().any(|line| line.len() > 32)
    {
        return Err(ApiError::new(
            "INVALID_CONFIG",
            "Simulation requires 1..16 lines of at most 32 moves",
            "Shorten or split the planning request.",
        ));
    }
    Ok(())
}

pub(crate) fn run(
    game: &dyn DynGame,
    initial: &replay::Frame,
    seed: u64,
    request: &SimulateRequest,
    viewer: Viewer,
) -> Result<SimulationResult, ApiError> {
    let mut lines = Vec::new();
    for actions in &request.lines {
        let mut frame = initial.clone();
        let mut result = SimulatedLine {
            moves_applied: 0,
            states: vec![],
            error: None,
        };
        for (index, action) in actions.iter().enumerate() {
            let attempted = step(game, &frame, action);
            match attempted {
                Ok(next) => {
                    frame = next;
                    result.moves_applied += 1;
                    if matches!(request.output, SimulationOutput::All) {
                        result.states.push(frame.project("", game, viewer)?);
                    }
                }
                Err(mut error) => {
                    let visible = frame.project("", game, viewer)?;
                    error.details =
                        json!({"turn":frame.turn,"legal_actions":visible.legal_actions});
                    result.error = Some(SimulationFailure { index, error });
                    break;
                }
            }
        }
        if matches!(request.output, SimulationOutput::Final) {
            result.states.push(frame.project("", game, viewer)?);
        }
        lines.push(result);
    }
    Ok(SimulationResult {
        game_id: game.spec().id,
        seed,
        lines,
    })
}

fn step(
    game: &dyn DynGame,
    frame: &replay::Frame,
    action: &Value,
) -> Result<replay::Frame, ApiError> {
    if frame.ended() {
        return Err(ApiError::new(
            "MATCH_FINISHED",
            "The simulated line has ended",
            "Stop this line or choose a different branch.",
        ));
    }
    let players = game.current_players(&frame.state).map_err(error::engine)?;
    if players.len() != 1 {
        return Err(ApiError::new(
            "INVALID_CONFIG",
            "Simulation requires sequential turns",
            "Use an engine with one current actor.",
        ));
    }
    let seat = players[0];
    let legal = game
        .legal_actions(&frame.state, seat)
        .map_err(error::engine)?;
    let (state, events) = game
        .apply(&frame.state, seat, action)
        .map_err(error::engine)?;
    if crate::service::canonical(&legal, action).is_none() {
        return Err(ApiError::new(
            "UNPARSEABLE_ACTION",
            "Action must use a canonical encoding",
            "Copy a string, json or index from legal_actions.",
        ));
    }
    replay::advance(game, state, frame.turn + 1, &events)
}

pub(crate) fn initial(
    game: &dyn DynGame,
    state: Value,
    turn: u64,
) -> Result<replay::Frame, ApiError> {
    Ok(replay::Frame {
        terminated: game.is_terminal(&state).map_err(error::engine)?,
        state,
        turn,
        truncated: false,
        outcome: None,
        draw_offer: None,
    })
}

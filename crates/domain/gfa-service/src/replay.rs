use crate::{error, MatchEvent, MatchOrigin, MatchRecord};
use gfa_api_types::{ApiError, MatchState, Start};
use gfa_core::{DynGame, GameRegistry, StepEvents, Viewer};
use serde_json::Value;
use std::sync::Arc;

pub(crate) struct Frame {
    pub state: Value,
    pub turn: u64,
    pub terminated: bool,
    pub truncated: bool,
}

impl Frame {
    pub fn ended(&self) -> bool {
        self.terminated || self.truncated
    }

    pub fn project(&self, id: &str, game: &dyn DynGame, viewer: Viewer) -> Result<MatchState, ApiError> {
        let (legal_actions, action_mask) = match viewer {
            Viewer::Player(seat) if !self.ended() => (
                game.legal_actions(&self.state, seat).map_err(error::engine)?,
                game.action_mask(&self.state, seat).map_err(error::engine)?,
            ),
            Viewer::Player(_) => (vec![], vec![false; game.spec().action_space_size as usize]),
            _ => (vec![], vec![]),
        };
        Ok(MatchState {
            match_id: id.into(),
            turn: self.turn,
            to_act: if self.ended() { vec![] } else {
                game.current_players(&self.state).map_err(error::engine)?
            },
            observation: game.observe(&self.state, viewer).map_err(error::engine)?,
            legal_actions,
            action_mask,
            returns: game.returns(&self.state).map_err(error::engine)?,
            terminated: self.terminated,
            truncated: self.truncated,
        })
    }
}

pub(crate) struct Reconstructed {
    pub game: Arc<dyn DynGame>,
    pub frames: Vec<Frame>,
}

impl Reconstructed {
    pub fn current(&self) -> Result<&Frame, ApiError> {
        self.frames.last().ok_or_else(error::corrupt)
    }
}

pub(crate) fn initial(game: &dyn DynGame, origin: &MatchOrigin) -> Result<Frame, ApiError> {
    // Validate configuration even when a custom state bypasses initial construction.
    let default = game.initial_state(&origin.config, origin.seed).map_err(error::engine)?;
    let state = match &origin.start {
        None => default,
        Some(Start::Position { position }) => game.state_from_notation(&origin.config, position).map_err(error::engine)?,
        Some(Start::State { state }) => {
            game.validate_state(state).map_err(error::engine)?;
            state.clone()
        }
    };
    if game.is_terminal(&state).map_err(error::engine)? || game.current_players(&state).map_err(error::engine)?.is_empty() {
        return Err(ApiError::new("INVALID_POSITION", "Starting position must be playable", "Choose a nonterminal position with an active seat."));
    }
    if game.spec().max_game_length == 0 {
        return Err(ApiError::new("INVALID_CONFIG", "Engine declares a zero game length", "Choose a playable engine."));
    }
    Ok(Frame { state, turn: 0, terminated: false, truncated: false })
}

pub(crate) fn advance(game: &dyn DynGame, state: Value, turn: u64, events: &StepEvents) -> Result<Frame, ApiError> {
    let terminated = game.is_terminal(&state).map_err(error::engine)?;
    let truncated = events.truncated || (!terminated && turn >= u64::from(game.spec().max_game_length));
    Ok(Frame { state, turn, terminated, truncated })
}

pub(crate) fn reconstruct(registry: &GameRegistry, record: &MatchRecord) -> Result<Reconstructed, ApiError> {
    let Some(MatchEvent::MatchCreated(origin)) = record.events.first() else {
        return Err(error::corrupt());
    };
    let game = registry.get(&origin.game_id).map_err(|_| ApiError::new(
        "ENGINE_UNAVAILABLE", "The recorded engine is not installed", "Install the game and version recorded with this match.",
    ))?;
    if game.spec().engine_version != origin.engine_version {
        return Err(ApiError::new("ENGINE_UNAVAILABLE", "The recorded engine version is unavailable", "Replay with the original engine version."));
    }
    if record.events.len() as u64 > u64::from(game.spec().max_game_length) + 2 {
        return Err(error::corrupt());
    }
    let mut result = Reconstructed { frames: vec![initial(game.as_ref(), origin).map_err(|_| error::corrupt())?], game };
    let mut finished = false;
    for event in record.events.iter().skip(1) {
        let current = result.current()?;
        if finished {
            return Err(error::corrupt());
        }
        match event {
            MatchEvent::MatchCreated(_) => return Err(error::corrupt()),
            MatchEvent::Action(action) => {
                if current.ended() || action.turn != current.turn {
                    return Err(error::corrupt());
                }
                let legal = result.game.legal_actions(&current.state, action.seat).map_err(|_| error::corrupt())?;
                if !legal.contains(&action.action) {
                    return Err(error::corrupt());
                }
                let (state, events) = result.game.apply(&current.state, action.seat, &action.action.json).map_err(|_| error::corrupt())?;
                if events != action.engine_events {
                    return Err(error::corrupt());
                }
                let frame = advance(result.game.as_ref(), state, current.turn + 1, &events).map_err(|_| error::corrupt())?;
                result.frames.push(frame);
            }
            MatchEvent::MatchFinished { turn, returns, terminated, truncated } => {
                if !current.ended() || *turn != current.turn || *terminated != current.terminated || *truncated != current.truncated
                    || *returns != result.game.returns(&current.state).map_err(|_| error::corrupt())? {
                    return Err(error::corrupt());
                }
                finished = true;
            }
        }
    }
    if result.current()?.ended() != finished {
        return Err(error::corrupt());
    }
    Ok(result)
}

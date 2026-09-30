//! Immutable seat assignments and atomic automatic replies.
use crate::{error, replay, AppliedAction, GameService, MatchEvent, MatchOrigin, MatchRecord};
use gfa_api_types::{ApiError, OpponentReply, Seat};
use gfa_core::{DynGame, Information, SeededRng, Viewer};
use std::sync::Arc;

pub(crate) struct AutomaticProgress {
    pub frame: replay::Frame,
    pub replies: Vec<OpponentReply>,
}

pub(crate) fn assignments(origin: &MatchOrigin, count: usize) -> Vec<Seat> {
    if origin.seats.is_empty() { vec![Seat::default(); count] } else { origin.seats.clone() }
}

pub(crate) fn external_seat(origin: &MatchOrigin, seat: u8) -> Result<(), ApiError> {
    if matches!(origin.seats.get(usize::from(seat)), Some(Seat::Opponent { .. })) {
        Err(ApiError::new("FORBIDDEN", "This seat is controlled by an opponent", "Submit moves for an external seat or fork with different assignments."))
    } else {
        Ok(())
    }
}

impl GameService {
    pub(crate) fn prepare_seats(&self, origin: &mut MatchOrigin, game: Arc<dyn DynGame>, frame: &replay::Frame) -> Result<(), ApiError> {
        let count = game.returns(&frame.state).map_err(error::engine)?.len();
        origin.seats = assignments(origin, count);
        if count == 0 || origin.seats.len() != count {
            return Err(ApiError::new("INVALID_CONFIG", "Seat assignments must match the game's actual player count", "Provide one seat per player, or omit seats for external play."));
        }
        // Interactive matches return control to an external player. Fully automatic
        // matches use the background runner added separately.
        if origin.seats.iter().all(|seat| matches!(seat, Seat::Opponent { .. })) {
            return Err(ApiError::new("INVALID_CONFIG", "An interactive match requires an external seat", "Include a self, human, or open seat."));
        }
        for seat in &mut origin.seats {
            if let Seat::Opponent { opponent, seed } = seat {
                let (factory, _) = self.opponents.as_ref().ok_or_else(|| ApiError::new(
                    "ENGINE_UNAVAILABLE", "No opponent executor is installed", "Configure opponent workers on the host."
                ))?;
                let planning_seed = *seed.get_or_insert_with(|| self.ids.next_seed());
                // Validate even an opponent that does not play first, before persistence.
                factory.create(game.clone(), origin.config.clone(), opponent, planning_seed)?;
            }
        }
        Ok(())
    }

    pub(crate) async fn automatic_replies(&self, staged: &mut MatchRecord) -> Result<AutomaticProgress, ApiError> {
        let origin = staged.origin().ok_or_else(error::corrupt)?.clone();
        let rebuilt = replay::reconstruct(&self.registry, staged)?;
        let mut frame = rebuilt.current()?.clone();
        let game = rebuilt.game;
        let mut replies = Vec::new();
        for _ in 0..8 {
            if frame.ended() { return Ok(AutomaticProgress { frame, replies }); }
            let actors = game.current_players(&frame.state).map_err(error::engine)?;
            let selected = actors.into_iter().find_map(|seat| match origin.seats.get(usize::from(seat)) {
                Some(Seat::Opponent { opponent, seed }) => Some((seat, opponent, seed)),
                _ => None,
            });
            let Some((seat, opponent, seed)) = selected else { return Ok(AutomaticProgress { frame, replies }); };
            // Planning randomness is independent of both live hidden state and game RNG.
            let base = seed.ok_or_else(error::corrupt)?;
            let planning_seed = SeededRng::new(base ^ frame.turn.wrapping_mul(0xd1342543de82ef95)).next_u64();
            let visible = frame.project(&staged.id, game.as_ref(), Viewer::Player(seat))?;
            let choice = self.choose_opponent(game.clone(), origin.config.clone(), opponent, planning_seed, seat, visible).await?;
            let (state, engine_events) = game.apply(&frame.state, seat, &choice.action.json).map_err(error::engine)?;
            let next = replay::advance(game.as_ref(), state, frame.turn + 1, &engine_events)?;
            replies.push(OpponentReply {
                seat, turn: frame.turn,
                action: (game.spec().information == Information::Perfect).then(|| choice.action.clone()),
            });
            staged.events.push(MatchEvent::Action(AppliedAction {
                turn: frame.turn, seat, action: choice.action, reasoning: None,
                opponent_info: Some(choice.info), engine_events, accepted_at_ms: self.clock.now_ms(),
            }));
            if next.ended() {
                staged.events.push(MatchEvent::MatchFinished {
                    turn: next.turn,
                    returns: game.returns(&next.state).map_err(error::engine)?,
                    terminated: next.terminated, truncated: next.truncated,
                });
            }
            frame = next;
        }
        if !frame.ended() && game.current_players(&frame.state).map_err(error::engine)?.iter().any(|seat| matches!(origin.seats.get(usize::from(*seat)), Some(Seat::Opponent { .. }))) {
            return Err(ApiError::new("ENGINE_UNAVAILABLE", "Automatic reply chain exceeded the interactive limit", "Use a shorter game or change the seat assignments."));
        }
        Ok(AutomaticProgress { frame, replies })
    }
}

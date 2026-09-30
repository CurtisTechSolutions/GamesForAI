//! Event projection is a domain operation shared by every transport.
use crate::{error, replay, AppliedAction, GameService, MatchEvent};
use gfa_api_types::{ApiError, EventData, EventPage, EventsQuery, RecordedEvent};
use gfa_core::{Information, Viewer};

impl GameService {
    /// Read one consistent log page with authorized action, reasoning, and chance-event visibility.
    pub async fn get_events(&self, id: &str, query: EventsQuery, viewer: Viewer) -> Result<EventPage, ApiError> {
        if !(1..=100).contains(&query.limit) {
            return Err(ApiError::new("INVALID_CONFIG", "Invalid event page size", "Use a limit from 1 through 100."));
        }
        if query.seat.is_some_and(|seat| viewer != Viewer::Player(seat)) {
            return Err(ApiError::new("FORBIDDEN", "Event query does not match the authorized viewer", "Use your authorized seat."));
        }
        let record = self.record(id).await?;
        let rebuilt = replay::reconstruct(&self.registry, &record)?;
        let origin = record.origin().ok_or_else(error::corrupt)?;
        let initial = replay::initial(rebuilt.game.as_ref(), origin)?.project(id, rebuilt.game.as_ref(), viewer)?;
        let start = match query.since {
            Some(sequence) if sequence < record.events.len() as u64 => sequence as usize + 1,
            Some(_) => return Err(ApiError::new("INVALID_CONFIG", "Event cursor is outside this log", "Use a sequence returned by a previous page.")),
            None => 0,
        };
        let end = start.saturating_add(query.limit as usize).min(record.events.len());
        let perfect = rebuilt.game.spec().information == Information::Perfect;
        let finished = rebuilt.current()?.ended();
        let mut events = Vec::new();
        for (offset, event) in record.events[start..end].iter().enumerate() {
            let event = match event {
                MatchEvent::MatchCreated(origin) => EventData::Created {
                    game_id:origin.game_id.clone(), engine_version:origin.engine_version.clone(),
                    config:rebuilt.game.normalize_config(&origin.config).map_err(error::engine)?,
                    seed:(viewer == Viewer::Omniscient).then_some(origin.seed),
                    seats:crate::seats::assignments(origin, initial.returns.len()),
                    assists:origin.assists.clone(), initial:Box::new(initial.clone()), at_ms:origin.created_at_ms,
                },
                MatchEvent::Action(action) => project_action(action, viewer, perfect, finished)?,
                MatchEvent::ForkedFrom { source, .. } => EventData::ForkedFrom { source:source.clone() },
                MatchEvent::Resigned { turn, seat, at_ms } => EventData::Resigned { turn:*turn, seat:*seat, at_ms:*at_ms },
                MatchEvent::DrawOffered { turn, seat, at_ms } => EventData::DrawOffered { turn:*turn, seat:*seat, at_ms:*at_ms },
                MatchEvent::MatchFinished { turn, returns, terminated, truncated } => EventData::Finished {
                    turn:*turn, returns:returns.clone(), terminated:*terminated, truncated:*truncated,
                },
            };
            events.push(RecordedEvent { sequence:(start+offset) as u64, event });
        }
        Ok(EventPage {
            match_id:id.into(), revision:record.events.len() as u64, events,
            next:(end < record.events.len()).then_some(end as u64 - 1),
        })
    }
}

fn project_action(action: &AppliedAction, viewer: Viewer, perfect: bool, finished: bool) -> Result<EventData, ApiError> {
    let own = viewer == Viewer::Player(action.seat) || viewer == Viewer::Omniscient;
    let explanations = own || (perfect && finished);
    Ok(EventData::Action {
        turn:action.turn, seat:action.seat,
        action:(own || perfect).then(|| action.action.clone()),
        reasoning:if explanations { action.reasoning.clone() } else { None },
        opponent_info:if explanations { action.opponent_info.as_ref().map(serde_json::to_value).transpose().map_err(|e| error::engine(e.into()))? } else { None },
        events:action.engine_events.events.iter().filter(|event| {
            viewer == Viewer::Omniscient || event.visible_to.as_ref().is_none_or(|seats| match viewer {
                Viewer::Player(seat) => seats.contains(&seat),
                _ => false,
            })
        }).cloned().collect(),
        at_ms:action.accepted_at_ms,
    })
}

//! Durable discovery and compare-and-append batches for host scheduling.
use crate::{error, replay, AppendResult, GameService, MatchOrigin};
use gfa_api_types::{ApiError, Seat};

/// One stored match the scheduler could not reconstruct.
#[derive(Debug)]
pub struct OpponentQueueFailure {
    /// Match requiring operator attention.
    pub match_id: String,
    /// Stable, sanitized failure context.
    pub error: ApiError,
}

/// Bounded cursor page for resuming automatic matches after host restart.
#[derive(Debug)]
pub struct PendingOpponents {
    /// Matches with an automatic player currently required to act.
    pub matches: Vec<String>,
    /// Exclusive cursor for the next page, including pages with no pending work.
    pub next: Option<String>,
    /// Bad or unavailable records do not prevent healthy matches from advancing.
    pub unavailable: Vec<OpponentQueueFailure>,
}

fn pending(origin: &MatchOrigin, actors: &[u8]) -> bool {
    actors.iter().any(|seat| matches!(origin.seats.get(usize::from(*seat)), Some(Seat::Opponent { .. })))
}

impl GameService {
    /// Discover pending opponents without requiring a client connection or notification.
    ///
    /// This is a trusted host operation over its owned match index.
    pub async fn pending_opponents(&self, after: Option<&str>, limit: u32) -> Result<PendingOpponents, ApiError> {
        let after = after.unwrap_or("");
        if !(1..=100).contains(&limit) || after.len() > 128 {
            return Err(ApiError::new("INVALID_CONFIG", "Invalid scheduler page", "Use a limit of 1..100 and the previous next cursor."));
        }
        let mut ids = self.store.list_ids(after, limit + 1).await.map_err(error::store)?;
        if ids.len() > limit as usize + 1 || ids.iter().any(|id| id.as_str() <= after)
            || ids.windows(2).any(|pair| pair[0] >= pair[1])
        { return Err(error::corrupt()); }
        let more = ids.len() > limit as usize;
        ids.truncate(limit as usize);
        let next = if more { ids.last().cloned() } else { None };
        let mut page = PendingOpponents { matches: vec![], next, unavailable: vec![] };
        for id in ids {
            match self.has_pending_opponent(&id).await {
                Ok(true) => page.matches.push(id),
                Ok(false) => {}
                Err(error) => page.unavailable.push(OpponentQueueFailure { match_id:id, error }),
            }
        }
        Ok(page)
    }

    async fn has_pending_opponent(&self, id: &str) -> Result<bool, ApiError> {
        let record = self.record(id).await?;
        let origin = record.origin().ok_or_else(error::corrupt)?;
        if !origin.seats.iter().any(|seat| matches!(seat, Seat::Opponent { .. })) { return Ok(false); }
        let rebuilt = replay::reconstruct(&self.registry, &record)?;
        let frame = rebuilt.current()?;
        Ok(!frame.ended() && pending(origin, &rebuilt.game.current_players(&frame.state).map_err(error::engine)?))
    }

    /// Commit up to eight automatic actions from the latest durable state.
    ///
    /// A competing host may return STALE_TURN; retrying discovers the next durable
    /// turn. Replay never reruns players. Hosts must coordinate paid providers with
    /// durable spending claims before installing those providers.
    pub async fn advance_opponents(&self, id: &str, limit: u32) -> Result<u32, ApiError> {
        if !(1..=8).contains(&limit) {
            return Err(ApiError::new("INVALID_CONFIG", "Invalid automatic move batch", "Use a limit of 1..8."));
        }
        let mut staged = self.record(id).await?;
        let revision = staged.events.len();
        let progress = self.automatic_replies(&mut staged, limit as usize).await?;
        let count = progress.replies.len() as u32;
        if count == 0 { return Ok(0); }
        let events = staged.events[revision..].to_vec();
        match self.store.append(id, revision as u64, events, None).await.map_err(error::store)? {
            AppendResult::Appended => {
                self.observer.committed(id);
                Ok(count)
            }
            AppendResult::AlreadyCommitted(_) => Err(error::corrupt()),
        }
    }
}

use crate::{error, replay, ForkAccess, GameService, MatchEvent, MatchOrigin, MatchRecord};
use gfa_api_types::{ApiError, CreatedMatch, ForkMatch, ForkSource, ReplayAncestor, Start};
use gfa_core::{Information, Viewer};
use std::collections::BTreeSet;

const MAX_ANCESTORS: usize = 32;
const MAX_HISTORY_STATES: usize = 50000;

fn denied(message: &str) -> ApiError {
    ApiError::new(
        "FORBIDDEN",
        message,
        "Use an authorized viewer or wait for the parent to finish.",
    )
}

impl GameService {
    /// Create a durable independent variation from one consistent parent snapshot.
    /// The host supplies authorization; none of these permissions comes from JSON.
    pub async fn fork_match(
        &self,
        id: &str,
        request: ForkMatch,
        access: ForkAccess,
    ) -> Result<CreatedMatch, ApiError> {
        if request.keep_rng && request.seed.is_some() {
            return Err(ApiError::new(
                "INVALID_CONFIG",
                "keep_rng and seed are mutually exclusive",
                "Choose preserved or fresh randomness.",
            ));
        }
        if access.viewer == Viewer::Omniscient && !access.full_state {
            return Err(denied(
                "Full-state access is required for an omniscient fork",
            ));
        }
        let parent = self.record(id).await?;
        let origin = parent.origin().ok_or_else(error::corrupt)?;
        let rebuilt = replay::reconstruct(&self.registry, &parent)?;
        let current = rebuilt.current()?;
        if !current.ended() && (!access.owns_parent || origin.benchmark_run.is_some()) {
            return Err(denied(
                "Only the owner may fork an active non-benchmark match",
            ));
        }
        if self.fork_ancestors(&parent).await?.len() >= MAX_ANCESTORS {
            return Err(ApiError::new(
                "INVALID_CONFIG",
                "Variation depth limit reached",
                "Export an authorized position to start a new root.",
            ));
        }
        let frame = rebuilt
            .frames
            .iter()
            .find(|frame| frame.turn == request.turn)
            .ok_or_else(|| {
                ApiError::new(
                    "INVALID_POSITION",
                    "The fork turn does not exist",
                    "Choose a local turn from the parent replay.",
                )
            })?;
        let game = rebuilt.game.as_ref();
        // Validate the requested seat before copying or sampling any state.
        let observation = game
            .observe(&frame.state, access.viewer)
            .map_err(error::engine)?;
        if request.keep_rng
            && game.spec().information == Information::Imperfect
            && !access.full_state
        {
            return Err(denied(
                "Preserving hidden randomness requires full-state access",
            ));
        }
        let seed = if request.keep_rng {
            origin.seed
        } else {
            request.seed.unwrap_or_else(|| self.ids.next_seed())
        };
        let state = if request.keep_rng {
            frame.state.clone()
        } else if access.full_state {
            game.reseed(&frame.state, seed).map_err(error::engine)?
        } else {
            game.state_from_observation(&origin.config, &observation, access.viewer, seed)
                .map_err(error::engine)?
        };
        let mut child_origin = MatchOrigin {
            game_id: origin.game_id.clone(),
            engine_version: origin.engine_version.clone(),
            config: origin.config.clone(),
            seed,
            start: Some(Start::State { state }),
            created_at_ms: self.clock.now_ms(),
            seats: request.seats.unwrap_or_else(|| origin.seats.clone()),
            assists: origin.assists.clone(),
            preserve_start_rng: request.keep_rng,
            benchmark_run: None,
        };
        let child = replay::initial(game, &child_origin)?;
        self.prepare_seats(&mut child_origin, rebuilt.game.clone(), &child)?;
        let child_id = self.ids.next_id();
        if child_id.is_empty() || child_id.len() > 128 {
            return Err(ApiError::new(
                "ID_CONFLICT",
                "Host generated an invalid match identifier",
                "Check the host identifier provider.",
            ));
        }
        let mut staged = MatchRecord {
            id: child_id.clone(),
            events: vec![
                MatchEvent::ForkedFrom {
                    source: ForkSource {
                        match_id: id.into(),
                        turn: request.turn,
                    },
                    viewer: access.viewer,
                    parent_revision: parent.events.len() as u64,
                },
                MatchEvent::MatchCreated(child_origin.clone()),
            ],
            commands: vec![],
        };
        let progress = self.automatic_replies(&mut staged, crate::seats::opening_budget(&child_origin)).await?;
        let state = progress.frame.project(&child_id, game, access.viewer)?;
        let info = if request.include_info {
            Some(
                crate::briefing::MatchBrief {
                    game,
                    origin: &child_origin,
                    initial: &child,
                    current: &progress.frame,
                    id: &child_id,
                    viewer: access.viewer,
                }
                .build(gfa_api_types::InfoDetail::Compact)?,
            )
        } else {
            None
        };
        self.store.create(staged).await.map_err(error::store)?;
        self.observer.committed(&state.match_id);
        Ok(CreatedMatch { state, info })
    }

    /// Return only public ancestor views, preserving source match IDs and local turns.
    pub(crate) async fn fork_ancestors(
        &self,
        record: &MatchRecord,
    ) -> Result<Vec<ReplayAncestor>, ApiError> {
        let mut child = record.clone();
        let mut seen = BTreeSet::from([record.id.clone()]);
        let mut result = Vec::new();
        let mut count = 0;
        while let Some(MatchEvent::ForkedFrom {
            source,
            viewer,
            parent_revision,
        }) = child.events.first()
        {
            if source.match_id.is_empty()
                || source.match_id.len() > 128
                || !seen.insert(source.match_id.clone())
                || result.len() >= MAX_ANCESTORS
            {
                return Err(error::corrupt());
            }
            let mut parent = self.record(&source.match_id).await?;
            let revision = usize::try_from(*parent_revision).map_err(|_| error::corrupt())?;
            if revision == 0 || revision > parent.events.len() {
                return Err(error::corrupt());
            }
            parent.events.truncate(revision);
            parent.commands.clear();
            let parent_origin = parent.origin().ok_or_else(error::corrupt)?;
            let child_origin = child.origin().ok_or_else(error::corrupt)?;
            if parent_origin.game_id != child_origin.game_id
                || parent_origin.engine_version != child_origin.engine_version
            {
                return Err(error::corrupt());
            }
            let replayed = replay::reconstruct(&self.registry, &parent)?;
            let frame = replayed
                .frames
                .iter()
                .find(|frame| frame.turn == source.turn)
                .ok_or_else(error::corrupt)?;
            let initial = replay::initial(replayed.game.as_ref(), child_origin)
                .map_err(|_| error::corrupt())?;
            let original = replayed
                .game
                .observe(&frame.state, *viewer)
                .map_err(|_| error::corrupt())?;
            let forked = replayed
                .game
                .observe(&initial.state, *viewer)
                .map_err(|_| error::corrupt())?;
            if original != forked {
                return Err(error::corrupt());
            }
            let frames: Vec<_> = replayed
                .frames
                .iter()
                .take_while(|frame| frame.turn <= source.turn)
                .collect();
            count += frames.len();
            if count > MAX_HISTORY_STATES {
                return Err(error::corrupt());
            }
            result.push(ReplayAncestor {
                source: source.clone(),
                engine_version: replayed.game.spec().engine_version,
                states: frames
                    .into_iter()
                    .map(|frame| {
                        frame.project(&parent.id, replayed.game.as_ref(), Viewer::Spectator)
                    })
                    .collect::<Result<_, _>>()?,
            });
            child = parent;
        }
        result.reverse();
        Ok(result)
    }
}

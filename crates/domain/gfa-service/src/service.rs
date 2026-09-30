use crate::{
    error, replay, AppendResult, AppliedAction, Clock, MatchEvent, MatchIds, MatchObserver,
    MatchOrigin, MatchRecord, MatchStore, StoreError, StoredCommand,
};
use gfa_api_types::{ApiError, CreateMatch, MatchState, MoveRequest, MoveResult, Replay};
use gfa_core::{GameRegistry, GameSpec, LegalAction, Viewer};
use serde_json::{json, Value};
use std::sync::Arc;

/// Match use cases with injected rules, persistence, time, and identifiers.
///
/// This in-process API trusts its host to authorize the selected seat and viewer.
/// Adapters must enforce that policy before accepting requests from the network.
pub struct GameService {
    pub(crate) registry: GameRegistry,
    pub(crate) store: Arc<dyn MatchStore>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Arc<dyn MatchIds>,
    pub(crate) observer: Arc<dyn MatchObserver>,
    pub(crate) opponents: Option<(
        Arc<dyn crate::OpponentFactory>,
        Arc<dyn crate::OpponentExecutor>,
    )>,
}

struct NoopObserver;
impl MatchObserver for NoopObserver {
    fn committed(&self, _: &str) {}
}

impl GameService {
    /// Construct without starting a runtime or performing I/O.
    pub fn new(
        registry: GameRegistry,
        store: Arc<dyn MatchStore>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn MatchIds>,
    ) -> Self {
        Self {
            registry,
            store,
            clock,
            ids,
            observer: Arc::new(NoopObserver),
            opponents: None,
        }
    }

    /// Install a best-effort observer before sharing the service with adapters.
    pub fn with_observer(mut self, observer: Arc<dyn MatchObserver>) -> Self {
        self.observer = observer;
        self
    }

    /// Registered games in stable identifier order.
    pub fn list_games(&self) -> Vec<GameSpec> {
        self.registry.specs()
    }

    /// Generate a deterministic model briefing from this engine's standard start.
    pub fn get_game_info(
        &self,
        game_id: &str,
        config: &Value,
        seat: Option<u8>,
        detail: gfa_api_types::InfoDetail,
    ) -> Result<gfa_api_types::Briefing, ApiError> {
        let game = self.registry.get(game_id).map_err(error::engine)?;
        crate::briefing::game_info(game.as_ref(), config, seat, detail)
    }

    /// Validate a standalone position and return canonical encodings without persistence.
    ///
    /// The caller supplies the complete input; this operation does not load match state.
    pub fn validate_position(
        &self,
        game_id: &str,
        request: gfa_api_types::ValidatePosition,
        viewer: Viewer,
    ) -> Result<gfa_api_types::ValidatedPosition, ApiError> {
        let game = self.registry.get(game_id).map_err(error::engine)?;
        let config = game
            .normalize_config(&request.config)
            .map_err(error::engine)?;
        let seed = request.seed.unwrap_or_else(|| self.ids.next_seed());
        // Parsing a complete state must not bypass config validation.
        game.initial_state(&config, seed).map_err(error::engine)?;
        let state = crate::position::import(game.as_ref(), &config, &request.start, seed)?;
        let terminated = game.is_terminal(&state).map_err(error::engine)?;
        let frame = replay::Frame {
            state: state.clone(),
            turn: 0,
            terminated,
            truncated: false,
            outcome: None,
            draw_offer: None,
        };
        let view = frame.project("", game.as_ref(), viewer)?;
        let spec = game.spec();
        Ok(gfa_api_types::ValidatedPosition {
            game_id: spec.id,
            engine_version: spec.engine_version,
            config,
            seed,
            position: game.state_to_notation(&state).map_err(error::engine)?,
            state,
            to_act: view.to_act,
            observation: view.observation,
            legal_actions: view.legal_actions,
            action_mask: view.action_mask,
            returns: view.returns,
            terminated,
        })
    }

    /// Create a match and return only its state for in-process callers.
    ///
    /// Transports use create_match_with_info to honor the include_info option.
    pub async fn create_match(
        &self,
        request: CreateMatch,
        viewer: Viewer,
    ) -> Result<MatchState, ApiError> {
        Ok(self.create(request, viewer, false).await?.state)
    }

    /// Create a match with an optional compact briefing prepared before persistence.
    pub async fn create_match_with_info(
        &self,
        request: CreateMatch,
        viewer: Viewer,
    ) -> Result<gfa_api_types::CreatedMatch, ApiError> {
        let include_info = request.include_info;
        self.create(request, viewer, include_info).await
    }

    async fn create(
        &self,
        request: CreateMatch,
        viewer: Viewer,
        include_info: bool,
    ) -> Result<gfa_api_types::CreatedMatch, ApiError> {
        let game = self.registry.get(&request.game_id).map_err(error::engine)?;
        let mut origin = MatchOrigin {
            game_id: request.game_id,
            engine_version: game.spec().engine_version,
            config: request.config,
            seed: request.seed.unwrap_or_else(|| self.ids.next_seed()),
            start: request.start,
            created_at_ms: self.clock.now_ms(),
            preserve_start_rng: false,
            benchmark_run: None,
            seats: request.seats,
            assists: request.assists,
        };
        let frame = replay::initial(game.as_ref(), &origin)?;
        self.prepare_seats(&mut origin, game.clone(), &frame)?;
        let id = self.ids.next_id();
        if id.is_empty() || id.len() > 128 {
            return Err(ApiError::new(
                "ID_CONFLICT",
                "Host generated an invalid match identifier",
                "Check the host identifier provider.",
            ));
        }
        // Complete bot openings on a private staging record before creating anything.
        let mut staged = MatchRecord {
            id: id.clone(),
            events: vec![MatchEvent::MatchCreated(origin.clone())],
            commands: vec![],
        };
        let progress = self
            .automatic_replies(&mut staged, crate::seats::opening_budget(&origin))
            .await?;
        let state = progress.frame.project(&id, game.as_ref(), viewer)?;
        let info = if include_info {
            Some(
                crate::briefing::MatchBrief {
                    game: game.as_ref(),
                    origin: &origin,
                    initial: &frame,
                    current: &progress.frame,
                    id: &id,
                    viewer,
                }
                .build(gfa_api_types::InfoDetail::Compact)?,
            )
        } else {
            None
        };
        self.store.create(staged).await.map_err(error::store)?;
        self.observer.committed(&state.match_id);
        Ok(gfa_api_types::CreatedMatch { state, info })
    }

    /// Read one consistent event snapshot and produce a viewer-scoped match briefing.
    pub async fn get_match_info(
        &self,
        id: &str,
        viewer: Viewer,
        detail: gfa_api_types::InfoDetail,
    ) -> Result<gfa_api_types::Briefing, ApiError> {
        let record = self.record(id).await?;
        let replay = replay::reconstruct(&self.registry, &record)?;
        let Some(origin) = record.origin() else {
            return Err(error::corrupt());
        };
        crate::briefing::MatchBrief {
            game: replay.game.as_ref(),
            origin,
            initial: replay.frames.first().ok_or_else(error::corrupt)?,
            current: replay.current()?,
            id,
            viewer,
        }
        .build(detail)
    }

    /// Read public metadata from one consistent event snapshot.
    pub async fn get_match(&self, id: &str) -> Result<gfa_api_types::MatchMetadata, ApiError> {
        let record = self.record(id).await?;
        let reconstructed = replay::reconstruct(&self.registry, &record)?;
        let Some(origin) = record.origin() else {
            return Err(error::corrupt());
        };
        let state =
            reconstructed
                .current()?
                .project(id, reconstructed.game.as_ref(), Viewer::Spectator)?;
        let seats = crate::seats::assignments(origin, state.returns.len());
        Ok(gfa_api_types::MatchMetadata {
            match_id: id.into(),
            forked_from: record.fork_source().cloned(),
            game_id: origin.game_id.clone(),
            engine_version: origin.engine_version.clone(),
            status: if state.terminated || state.truncated {
                gfa_api_types::MatchStatus::Finished
            } else {
                gfa_api_types::MatchStatus::Active
            },
            turn: state.turn,
            to_act: state.to_act,
            returns: state.returns,
            terminated: state.terminated,
            truncated: state.truncated,
            created_at_ms: origin.created_at_ms,
            seats,
            assists: origin.assists.clone(),
            assist_usage: self.store.assist_usage(id).await.map_err(error::store)?,
            outcome: state.outcome,
            draw_offer: state.draw_offer,
        })
    }

    /// Scan a bounded page of local match history; callers must follow next even
    /// when filters leave this page empty. Concurrent insertions before a cursor
    /// appear on a new scan, preserving stable forward pagination.
    pub async fn list_matches(
        &self,
        query: gfa_api_types::MatchHistoryQuery,
    ) -> Result<gfa_api_types::MatchHistory, ApiError> {
        if !(1..=100).contains(&query.limit)
            || query.after.as_ref().is_some_and(|value| value.len() > 128)
        {
            return Err(ApiError::new(
                "INVALID_CONFIG",
                "Invalid history limit or cursor",
                "Use a limit from 1 to 100 and the previous page's next cursor.",
            ));
        }
        if let Some(game_id) = &query.game_id {
            self.registry.get(game_id).map_err(error::engine)?;
        }
        let after = query.after.as_deref().unwrap_or("");
        let mut ids = self
            .store
            .list_ids(after, query.limit + 1)
            .await
            .map_err(error::store)?;
        if ids.len() > query.limit as usize + 1
            || ids.iter().any(|id| id.as_str() <= after)
            || ids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(error::corrupt());
        }
        let more = ids.len() > query.limit as usize;
        ids.truncate(query.limit as usize);
        let next = if more { ids.last().cloned() } else { None };
        let mut matches = Vec::new();
        for id in ids {
            let metadata = self.get_match(&id).await?;
            if query
                .game_id
                .as_ref()
                .is_none_or(|id| id == &metadata.game_id)
                && query.status.is_none_or(|status| status == metadata.status)
            {
                matches.push(metadata);
            }
        }
        Ok(gfa_api_types::MatchHistory { matches, next })
    }

    /// Reconstruct and project the latest committed state.
    pub async fn get_state(&self, id: &str, viewer: Viewer) -> Result<MatchState, ApiError> {
        let record = self.record(id).await?;
        let replay = replay::reconstruct(&self.registry, &record)?;
        replay.current()?.project(id, replay.game.as_ref(), viewer)
    }

    /// Replay every committed action with the original version, config, and seed.
    pub async fn get_replay(&self, id: &str, viewer: Viewer) -> Result<Replay, ApiError> {
        let record = self.record(id).await?;
        let replay = replay::reconstruct(&self.registry, &record)?;
        let origin = record.origin().ok_or_else(error::corrupt)?;
        let config = replay.game.normalize_config(&origin.config).map_err(error::engine)?;
        let mut budget = crate::events::ReplayBudget::default();
        budget.include(&config)?;
        let initial_state = if viewer == Viewer::Omniscient {
            let state = replay.frames.first().ok_or_else(error::corrupt)?.state.clone();
            budget.include(&state)?;
            Some(state)
        } else { None };
        let mut states = Vec::new();
        for frame in &replay.frames {
            let state = frame.project(id, replay.game.as_ref(), viewer)?;
            budget.include(&state)?;
            states.push(state);
        }
        let events = crate::events::project(&record, &replay, viewer, 0..record.events.len(), &mut budget)?;
        let ancestors = self.fork_ancestors(&record).await?;
        budget.include(&ancestors)?;
        Ok(Replay {
            game_id:origin.game_id.clone(), config,
            seed:(viewer == Viewer::Omniscient).then_some(origin.seed),
            initial_state, revision:record.events.len() as u64, events,
            match_id: id.into(),
            engine_version: replay.game.spec().engine_version,
            states,
            ancestors,
        })
    }

    /// Apply one move atomically against its expected turn.
    ///
    /// A key is scoped to this match. Identical retries return the original response,
    /// even after later turns; reusing the key with different input is rejected.
    pub async fn make_move(
        &self,
        id: &str,
        request: MoveRequest,
        idempotency_key: Option<&str>,
    ) -> Result<MoveResult, ApiError> {
        validate_command(&request, idempotency_key)?;
        let record = self.record(id).await?;
        if let Some(key) = idempotency_key {
            if let Some(previous) = record.commands.iter().find(|command| command.key == key) {
                return if previous.request == request {
                    Ok(previous.response.clone())
                } else {
                    Err(error::store(StoreError::IdempotencyConflict))
                };
            }
        }
        crate::seats::external_seat(record.origin().ok_or_else(error::corrupt)?, request.seat)?;
        let reconstructed = replay::reconstruct(&self.registry, &record)?;
        let game = reconstructed.game.as_ref();
        let current = reconstructed.current()?;
        let view = current.project(id, game, Viewer::Player(request.seat))?;
        let with_context = |mut error: ApiError| {
            error.details = json!({ "turn": view.turn, "to_act": view.to_act, "legal_actions": view.legal_actions });
            error
        };
        if request.turn != current.turn {
            return Err(with_context(error::store(StoreError::Conflict)));
        }
        if current.ended() {
            return Err(with_context(ApiError::new(
                "MATCH_FINISHED",
                "This match has ended",
                "Create or fork a playable match.",
            )));
        }
        if !view.to_act.contains(&request.seat) {
            return Err(with_context(ApiError::new(
                "NOT_YOUR_TURN",
                "This seat cannot act now",
                "Wait for your seat to appear in to_act.",
            )));
        }
        let (state, engine_events) = game
            .apply(&current.state, request.seat, &request.action)
            .map_err(|error| with_context(error::engine(error)))?;
        let accepted_action = canonical(&view.legal_actions, &request.action).ok_or_else(|| {
            with_context(ApiError::new(
                "UNPARSEABLE_ACTION",
                "Action must use a listed canonical encoding",
                "Copy a string, json, or index from legal_actions.",
            ))
        })?;
        let next = replay::advance(game, state, current.turn + 1, &engine_events)?;
        let mut response = MoveResult {
            opponent_actions: vec![],
            accepted_action: accepted_action.clone(),
            state: next.project(id, game, Viewer::Player(request.seat))?,
        };
        let mut events = vec![MatchEvent::Action(AppliedAction {
            turn: current.turn,
            seat: request.seat,
            action: accepted_action,
            reasoning: request.reasoning.clone(),
            opponent_info: None,
            engine_events,
            accepted_at_ms: self.clock.now_ms(),
        })];
        if next.ended() {
            events.push(MatchEvent::MatchFinished {
                turn: next.turn,
                returns: response.state.returns.clone(),
                terminated: next.terminated,
                truncated: next.truncated,
            });
        }
        let revision = record.events.len();
        let mut staged = record.clone();
        staged.events.extend(events);
        let progress = self.automatic_replies(&mut staged, 8).await?;
        response.state = progress
            .frame
            .project(id, game, Viewer::Player(request.seat))?;
        response.opponent_actions = progress.replies;
        let events = staged.events[revision..].to_vec();
        let command = idempotency_key.map(|key| StoredCommand {
            key: key.into(),
            request: request.clone(),
            response: response.clone(),
        });
        match self
            .store
            .append(id, record.events.len() as u64, events, command)
            .await
        {
            Ok(AppendResult::Appended) => {
                self.observer.committed(id);
                Ok(response)
            }
            Ok(AppendResult::AlreadyCommitted(original)) => Ok(*original),
            Err(StoreError::Conflict) => {
                let mut error = error::store(StoreError::Conflict);
                if let Ok(latest) = self.get_state(id, Viewer::Player(request.seat)).await {
                    error.details = json!({ "turn": latest.turn, "to_act": latest.to_act, "legal_actions": latest.legal_actions });
                }
                Err(error)
            }
            Err(error) => Err(error::store(error)),
        }
    }

    /// Simulate independent lines from caller input or an authorized observation.
    pub async fn simulate(
        &self,
        game_id: &str,
        request: gfa_api_types::SimulateRequest,
        viewer: Viewer,
    ) -> Result<gfa_api_types::SimulationResult, ApiError> {
        crate::simulation::validate(&request)?;
        let seed = request.seed.unwrap_or_else(|| self.ids.next_seed());
        let planning = self
            .planning_source(
                game_id,
                &request.from,
                &request.config,
                seed,
                viewer,
                crate::planning::AssistKind::Simulation,
            )
            .await?;
        let result = crate::simulation::run(
            planning.game.as_ref(),
            &planning.frame,
            seed,
            &request,
            viewer,
        )?;
        if let Some((id, seat)) = planning.accounting {
            let moves = result.lines.iter().map(|line| line.moves_applied).sum();
            self.store
                .record_simulation(&id, seat, moves)
                .await
                .map_err(error::store)?;
        }
        Ok(result)
    }

    /// Resign a one- or two-seat match, preserving its engine position.
    pub async fn resign(
        &self,
        id: &str,
        request: gfa_api_types::ControlRequest,
    ) -> Result<MatchState, ApiError> {
        self.control(id, request, true).await
    }

    /// Offer a draw; the other seat's offer at the same turn accepts it.
    /// Repeating one's own offer is harmless. Any subsequent move expires it.
    pub async fn offer_draw(
        &self,
        id: &str,
        request: gfa_api_types::ControlRequest,
    ) -> Result<MatchState, ApiError> {
        self.control(id, request, false).await
    }

    async fn control(
        &self,
        id: &str,
        request: gfa_api_types::ControlRequest,
        resign: bool,
    ) -> Result<MatchState, ApiError> {
        let mut record = self.record(id).await?;
        let reconstructed = replay::reconstruct(&self.registry, &record)?;
        let current = reconstructed.current()?;
        let state = current.project(
            id,
            reconstructed.game.as_ref(),
            Viewer::Player(request.seat),
        )?;
        if request.turn != current.turn {
            let mut failure = error::store(StoreError::Conflict);
            failure.details = json!({"turn":current.turn});
            return Err(failure);
        }
        if current.ended() {
            return Err(ApiError::new(
                "MATCH_FINISHED",
                "This match has ended",
                "Create or fork a match.",
            ));
        }
        let count = state.returns.len();
        if count > 2 || (!resign && count != 2) {
            return Err(ApiError::new(
                "INVALID_CONFIG",
                "Control is unsupported for this player count",
                "Draws require two seats; resignation supports one or two seats.",
            ));
        }
        if !resign && state.draw_offer == Some(request.seat) {
            return Ok(state);
        }
        let event = if resign {
            MatchEvent::Resigned {
                turn: request.turn,
                seat: request.seat,
                at_ms: self.clock.now_ms(),
            }
        } else {
            MatchEvent::DrawOffered {
                turn: request.turn,
                seat: request.seat,
                at_ms: self.clock.now_ms(),
            }
        };
        let revision = record.events.len() as u64;
        record.events.push(event.clone());
        let after = replay::reconstruct(&self.registry, &record)?;
        let response =
            after
                .current()?
                .project(id, after.game.as_ref(), Viewer::Player(request.seat))?;
        self.store
            .append(id, revision, vec![event], None)
            .await
            .map_err(error::store)?;
        self.observer.committed(id);
        Ok(response)
    }

    pub(crate) async fn record(&self, id: &str) -> Result<MatchRecord, ApiError> {
        let record = self
            .store
            .load(id)
            .await
            .map_err(error::store)?
            .ok_or_else(error::missing)?;
        if record.id != id {
            return Err(error::corrupt());
        }
        Ok(record)
    }
}

pub(crate) fn canonical(legal: &[LegalAction], input: &Value) -> Option<LegalAction> {
    legal
        .iter()
        .find(|action| {
            input == &Value::String(action.string.clone())
                || input == &action.json
                || input == &json!({ "index": action.index })
        })
        .cloned()
}

fn validate_command(request: &MoveRequest, key: Option<&str>) -> Result<(), ApiError> {
    if key.is_some_and(|key| key.is_empty() || key.len() > 128) {
        return Err(ApiError::new(
            "INVALID_CONFIG",
            "Idempotency key must contain 1 to 128 bytes",
            "Use a short opaque key for this command.",
        ));
    }
    if request
        .reasoning
        .as_ref()
        .is_some_and(|reasoning| reasoning.len() > 8192)
    {
        return Err(ApiError::new(
            "INVALID_CONFIG",
            "Reasoning exceeds 8192 bytes",
            "Shorten the explanation and retry.",
        ));
    }
    Ok(())
}

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
    registry: GameRegistry,
    store: Arc<dyn MatchStore>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn MatchIds>,
    observer: Arc<dyn MatchObserver>,
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

    /// Validate inputs and persist an immutable match origin.
    pub async fn create_match(
        &self,
        request: CreateMatch,
        viewer: Viewer,
    ) -> Result<MatchState, ApiError> {
        let game = self.registry.get(&request.game_id).map_err(error::engine)?;
        let origin = MatchOrigin {
            game_id: request.game_id,
            engine_version: game.spec().engine_version,
            config: request.config,
            seed: request.seed.unwrap_or_else(|| self.ids.next_seed()),
            start: request.start,
            created_at_ms: self.clock.now_ms(),
        };
        let frame = replay::initial(game.as_ref(), &origin)?;
        let id = self.ids.next_id();
        if id.is_empty() || id.len() > 128 {
            return Err(ApiError::new(
                "ID_CONFLICT",
                "Host generated an invalid match identifier",
                "Check the host identifier provider.",
            ));
        }
        let state = frame.project(&id, game.as_ref(), viewer)?;
        self.store
            .create(MatchRecord {
                id,
                events: vec![MatchEvent::MatchCreated(origin)],
                commands: vec![],
            })
            .await
            .map_err(error::store)?;
        self.observer.committed(&state.match_id);
        Ok(state)
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
        let states = replay
            .frames
            .iter()
            .map(|frame| frame.project(id, replay.game.as_ref(), viewer))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Replay {
            match_id: id.into(),
            engine_version: replay.game.spec().engine_version,
            states,
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
        let response = MoveResult {
            accepted_action: accepted_action.clone(),
            state: next.project(id, game, Viewer::Player(request.seat))?,
        };
        let mut events = vec![MatchEvent::Action(AppliedAction {
            turn: current.turn,
            seat: request.seat,
            action: accepted_action,
            reasoning: request.reasoning.clone(),
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

    async fn record(&self, id: &str) -> Result<MatchRecord, ApiError> {
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

fn canonical(legal: &[LegalAction], input: &Value) -> Option<LegalAction> {
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

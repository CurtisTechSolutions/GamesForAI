use super::*;
use gfa_api_types::{ApiError, CreateMatch, MoveRequest, Start};
use gfa_core::{
    schema, ErrorCode, Game, GameError, GameEvent, GameRegistry, GameSpec, Information,
    Observation, PlayerId, StepEvents, TurnStructure, Viewer,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// A tiny stochastic engine exercises the service without concrete game dependencies.
struct Counter<const TERMINATES: bool, const PUBLIC_METADATA: bool = false>;

impl<const TERMINATES: bool, const PUBLIC_METADATA: bool> Game
    for Counter<TERMINATES, PUBLIC_METADATA>
{
    type State = [u64; 3]; // turn, random state, public total
    type Action = u8;
    type Config = Value;

    fn spec() -> GameSpec {
        GameSpec {
            id: if TERMINATES { "counter" } else { "capped" }.into(),
            name: "Counter".into(),
            summary: "Service test fixture".into(),
            engine_version: "1.0.0".into(),
            info_version: "1.0.0".into(),
            num_players: [2, 2],
            seat_names: vec!["A".into(), "B".into()],
            turn_structure: TurnStructure::Sequential,
            information: if PUBLIC_METADATA {
                Information::Perfect
            } else {
                Information::Imperfect
            },
            stochastic: !PUBLIC_METADATA,
            max_game_length: 4,
            reward_range: [-1.0, 1.0],
            action_space_size: 2,
            rules_markdown: "Take four turns.".into(),
            action_notation: "1 or 2".into(),
            position_notation: "JSON array".into(),
            config_schema: schema::<Value>(),
            action_schema: schema::<u8>(),
            observation_schema: schema::<Value>(),
        }
    }
    fn play_guide() -> Option<gfa_core::PlayGuide> {
        // The capped fixture intentionally has no guide, to test atomic failures.
        TERMINATES.then(|| gfa_core::PlayGuide {
            objective: "Take four turns.".into(),
            observation: "Public turn and total; only a player's own private label is visible."
                .into(),
            tensor: "No tensor.".into(),
            rewards: "Terminal +1/-1.".into(),
            config: "No options.".into(),
            solved: "Not applicable to this fixture.".into(),
            common_mistakes: vec!["Only 1 and 2 are valid.".into()],
            strategy_notes: vec![],
        })
    }

    fn new_initial_state(config: &Value, seed: u64) -> Result<Self::State, GameError> {
        if !config.is_null() && config != &json!({}) {
            return Err(GameError::new(
                ErrorCode::InvalidConfig,
                "Unknown option",
                "Use empty config.",
            ));
        }
        Ok([0, seed, 0])
    }
    fn reseed(state: &mut Self::State, seed: u64) -> Result<(), GameError> {
        state[1] = seed;
        Ok(())
    }
    fn state_from_observation(
        config: &Self::Config,
        observation: &Observation,
        _: Viewer,
        seed: u64,
    ) -> Result<Self::State, GameError> {
        Self::new_initial_state(config, seed)?;
        let turn = observation.json["turn"]
            .as_u64()
            .ok_or_else(|| GameError::position("turn"))?;
        let total = observation.json["total"]
            .as_u64()
            .ok_or_else(|| GameError::position("total"))?;
        let state = [turn, seed, total];
        Self::validate_state(&state)?;
        Ok(state)
    }
    fn validate_state(state: &Self::State) -> Result<(), GameError> {
        if state[0] > 4 || state[2] > state[0] * 3 {
            return Err(GameError::position("Invalid counter"));
        }
        Ok(())
    }
    fn current_players(state: &Self::State) -> Vec<PlayerId> {
        if Self::is_terminal(state) {
            vec![]
        } else {
            vec![(state[0] % 2) as u8]
        }
    }
    fn legal_actions(state: &Self::State, player: PlayerId) -> Vec<u8> {
        if Self::current_players(state).contains(&player) {
            vec![1, 2]
        } else {
            vec![]
        }
    }
    fn apply(
        state: &mut Self::State,
        player: PlayerId,
        action: &u8,
    ) -> Result<StepEvents, GameError> {
        if !Self::current_players(state).contains(&player) {
            return Err(GameError::new(
                ErrorCode::NotYourTurn,
                "Wrong seat",
                "Wait.",
            ));
        }
        if !(1..=2).contains(action) {
            return Err(GameError::illegal("Use 1 or 2"));
        }
        state[1] = state[1].wrapping_mul(6364136223846793005).wrapping_add(1);
        let roll = state[1] % 2;
        state[0] += 1;
        state[2] += u64::from(*action) + roll;
        Ok(StepEvents {
            events: vec![GameEvent {
                kind: "chance".into(),
                payload: json!(roll),
                visible_to: Some(vec![player]),
            }],
            truncated: false,
        })
    }
    fn is_terminal(state: &Self::State) -> bool {
        TERMINATES && state[0] == 4
    }
    fn returns(state: &Self::State) -> Vec<f64> {
        if Self::is_terminal(state) {
            vec![1.0, -1.0]
        } else {
            vec![0.0, 0.0]
        }
    }
    fn observe(state: &Self::State, viewer: Viewer) -> Observation {
        let private = match viewer {
            Viewer::Player(seat) => json!(format!("seat-{seat}")),
            Viewer::Spectator => Value::Null,
            Viewer::Omniscient => json!(state[1]),
        };
        Observation {
            text: format!("Turn {}", state[0]),
            json: json!({ "turn": state[0], "total": state[2], "private": private }),
            tensor: None,
        }
    }
    fn action_to_string(_: &Self::State, action: &u8) -> String {
        action.to_string()
    }
    fn action_from_string(_: &Self::State, text: &str) -> Result<u8, GameError> {
        text.parse().map_err(|_| {
            GameError::new(
                ErrorCode::UnparseableAction,
                "Invalid action",
                "Use 1 or 2.",
            )
        })
    }
    fn action_to_index(action: &u8) -> u32 {
        u32::from(*action - 1)
    }
    fn action_from_index(_: &Self::State, index: u32) -> Result<u8, GameError> {
        match index {
            0 => Ok(1),
            1 => Ok(2),
            _ => Err(GameError::illegal("Invalid index")),
        }
    }
    fn state_to_notation(state: &Self::State) -> Result<String, GameError> {
        Ok(serde_json::to_string(state)?)
    }
    fn state_from_notation(_: &Value, notation: &str) -> Result<Self::State, GameError> {
        let state = serde_json::from_str(notation)?;
        Self::validate_state(&state)?;
        Ok(state)
    }
}

type SimulationUsage = BTreeMap<String, BTreeMap<u8, (u64, u64)>>;

#[derive(Default)]
struct MemoryStore {
    records: Mutex<BTreeMap<String, MatchRecord>>,
    usage: Mutex<SimulationUsage>,
    // Inject a failure (1), identical command race (2), or unkeyed move race (3).
    append_mode: AtomicU8,
}

impl MatchStore for MemoryStore {
    fn create(&self, record: MatchRecord) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            let mut records = self
                .records
                .lock()
                .map_err(|_| StoreError::Unavailable("poisoned".into()))?;
            if records.contains_key(&record.id) {
                return Err(StoreError::DuplicateId);
            }
            records.insert(record.id.clone(), record);
            Ok(())
        })
    }
    fn load<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Option<MatchRecord>> {
        Box::pin(async move {
            Ok(self
                .records
                .lock()
                .map_err(|_| StoreError::Unavailable("poisoned".into()))?
                .get(id)
                .cloned())
        })
    }
    fn list_ids<'a>(&'a self, after: &'a str, limit: u32) -> StoreFuture<'a, Vec<String>> {
        Box::pin(async move {
            Ok(self
                .records
                .lock()
                .map_err(|_| StoreError::Unavailable("poisoned".into()))?
                .keys()
                .filter(|id| id.as_str() > after)
                .take(limit as usize)
                .cloned()
                .collect())
        })
    }
    fn record_simulation<'a>(&'a self, id: &'a str, seat: u8, moves: u32) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let mut usage = self
                .usage
                .lock()
                .map_err(|_| StoreError::Unavailable("poisoned".into()))?;
            let entry = usage.entry(id.into()).or_default().entry(seat).or_default();
            entry.0 += 1;
            entry.1 += u64::from(moves);
            Ok(())
        })
    }
    fn assist_usage<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Vec<gfa_api_types::AssistUsage>> {
        Box::pin(async move {
            let usage = self
                .usage
                .lock()
                .map_err(|_| StoreError::Unavailable("poisoned".into()))?;
            Ok(usage
                .get(id)
                .into_iter()
                .flat_map(|seats| seats.iter())
                .map(|(seat, (calls, moves))| gfa_api_types::AssistUsage {
                    seat: *seat,
                    simulation_calls: *calls,
                    simulated_moves: *moves,
                })
                .collect())
        })
    }
    fn append<'a>(
        &'a self,
        id: &'a str,
        expected_revision: u64,
        events: Vec<MatchEvent>,
        command: Option<StoredCommand>,
    ) -> StoreFuture<'a, AppendResult> {
        Box::pin(async move {
            let mut records = self
                .records
                .lock()
                .map_err(|_| StoreError::Unavailable("poisoned".into()))?;
            let record = records.get_mut(id).ok_or(StoreError::NotFound)?;
            match self.append_mode.swap(0, Ordering::SeqCst) {
                1 => return Err(StoreError::Unavailable("private adapter diagnostic".into())),
                2 => {
                    record.events.extend(events.clone());
                    record.commands.extend(command.clone());
                }
                3 => record.events.extend(events.clone()),
                _ => {}
            }
            if let Some(command) = &command {
                if let Some(previous) = record
                    .commands
                    .iter()
                    .find(|previous| previous.key == command.key)
                {
                    return if previous.request == command.request {
                        Ok(AppendResult::AlreadyCommitted(Box::new(
                            previous.response.clone(),
                        )))
                    } else {
                        Err(StoreError::IdempotencyConflict)
                    };
                }
            }
            if record.events.len() as u64 != expected_revision {
                return Err(StoreError::Conflict);
            }
            record.events.extend(events);
            record.commands.extend(command);
            Ok(AppendResult::Appended)
        })
    }
}

#[derive(Default)]
struct Host(AtomicU64);
impl Clock for Host {
    fn now_ms(&self) -> u64 {
        1234
    }
}
impl MatchIds for Host {
    fn next_id(&self) -> String {
        format!("match-{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
    fn next_seed(&self) -> u64 {
        42
    }
}

fn run<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::noop();
    match std::pin::pin!(future)
        .as_mut()
        .poll(&mut Context::from_waker(waker))
    {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("The in-memory fake must complete immediately"),
    }
}
fn fixture() -> Result<(GameService, Arc<MemoryStore>), GameError> {
    let mut registry = GameRegistry::default();
    registry.register::<Counter<true>>()?;
    registry.register::<Counter<false>>()?;
    let store = Arc::new(MemoryStore::default());
    let host = Arc::new(Host::default());
    Ok((
        GameService::new(registry, store.clone(), host.clone(), host),
        store,
    ))
}
fn create(game: &str) -> CreateMatch {
    CreateMatch {
        game_id: game.into(),
        config: json!({}),
        seed: Some(7),
        start: None,
        include_info: true,
        seats: vec![],
        assists: gfa_api_types::Assists::default(),
    }
}
fn action(turn: u64, value: Value) -> MoveRequest {
    MoveRequest {
        seat: (turn % 2) as u8,
        turn,
        action: value,
        reasoning: Some("test move".into()),
    }
}
fn record(store: &MemoryStore, id: &str) -> Result<MatchRecord, Box<dyn std::error::Error>> {
    run(store.load(id))?.ok_or_else(|| "missing test record".into())
}
fn code<T: std::fmt::Debug>(result: Result<T, ApiError>, expected: &str) {
    match result {
        Err(error) => assert_eq!(error.code, expected),
        Ok(value) => panic!("Expected {expected}, got {value:?}"),
    }
}

#[test]
fn lifecycle_replays_chance_and_all_action_encodings() -> TestResult {
    let (service, store) = fixture()?;
    let initial = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let id = &initial.match_id;
    let mut expected = vec![initial.clone()];
    for (turn, value) in [json!("1"), json!(2), json!({"index": 0}), json!("2")]
        .into_iter()
        .enumerate()
    {
        let result = run(service.make_move(id, action(turn as u64, value), None))?;
        assert_eq!(result.state.turn, turn as u64 + 1);
        expected.push(run(service.get_state(id, Viewer::Player(0)))?);
    }
    let replay = run(service.get_replay(id, Viewer::Player(0)))?;
    assert_eq!(replay.states, expected);
    let final_state = run(service.get_state(id, Viewer::Player(0)))?;
    assert!(final_state.terminated);
    assert!(!final_state.truncated);
    assert!(final_state.to_act.is_empty());
    assert_eq!(final_state.action_mask, vec![false, false]);
    assert_eq!(final_state.returns, vec![1.0, -1.0]);
    assert_eq!(record(&store, id)?.events.len(), 6);
    code(
        run(service.make_move(id, action(4, json!("1")), None)),
        "MATCH_FINISHED",
    );
    Ok(())
}

#[test]
fn invalid_moves_and_failed_storage_do_not_change_events_or_receipts() -> TestResult {
    let (service, store) = fixture()?;
    let initial = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let id = &initial.match_id;
    let before = record(&store, id)?;
    let mut wrong_seat = action(0, json!("1"));
    wrong_seat.seat = 1;
    code(
        run(service.make_move(id, wrong_seat, None)),
        "NOT_YOUR_TURN",
    );
    code(
        run(service.make_move(id, action(2, json!("1")), None)),
        "STALE_TURN",
    );
    code(
        run(service.make_move(id, action(0, json!("9")), None)),
        "ILLEGAL_ACTION",
    );
    code(
        run(service.make_move(id, action(0, json!("banana")), None)),
        "UNPARSEABLE_ACTION",
    );
    code(
        run(service.make_move(id, action(0, json!("01")), None)),
        "UNPARSEABLE_ACTION",
    );
    code(
        run(service.make_move(id, action(0, json!("1")), Some(""))),
        "INVALID_CONFIG",
    );
    assert_eq!(record(&store, id)?, before);
    store.append_mode.store(1, Ordering::SeqCst);
    match run(service.make_move(id, action(0, json!("1")), Some("retry"))) {
        Err(error) => {
            assert_eq!(error.code, "STORAGE_UNAVAILABLE");
            assert!(!error.message.contains("private"));
        }
        Ok(_) => return Err("storage failure unexpectedly succeeded".into()),
    }
    assert_eq!(record(&store, id)?, before);
    assert_eq!(run(service.get_state(id, Viewer::Player(0)))?, initial);
    run(service.make_move(id, action(0, json!("1")), Some("retry")))?;
    Ok(())
}

#[test]
fn idempotent_retries_return_original_response_after_later_turns() -> TestResult {
    let (service, store) = fixture()?;
    let initial = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let id = &initial.match_id;
    let request = action(0, json!("1"));
    let first = run(service.make_move(id, request.clone(), Some("once")))?;
    run(service.make_move(id, action(1, json!("2")), None))?;
    let before = record(&store, id)?;
    assert_eq!(run(service.make_move(id, request, Some("once")))?, first);
    code(
        run(service.make_move(id, action(0, json!("2")), Some("once"))),
        "IDEMPOTENCY_CONFLICT",
    );
    assert_eq!(record(&store, id)?, before);
    assert_eq!(run(service.get_state(id, Viewer::Player(0)))?.turn, 2);
    Ok(())
}

#[test]
fn compare_and_append_handles_both_racing_retries_and_new_moves() -> TestResult {
    let (service, store) = fixture()?;
    let first = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    store.append_mode.store(2, Ordering::SeqCst);
    let result = run(service.make_move(&first.match_id, action(0, json!("1")), Some("race")))?;
    assert_eq!(result.state.turn, 1);
    assert_eq!(record(&store, &first.match_id)?.events.len(), 2);
    assert_eq!(record(&store, &first.match_id)?.commands.len(), 1);
    let second = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    store.append_mode.store(3, Ordering::SeqCst);
    match run(service.make_move(&second.match_id, action(0, json!("1")), Some("loser"))) {
        Err(error) => {
            assert_eq!(error.code, "STALE_TURN");
            assert_eq!(error.details["turn"], 1);
        }
        Ok(_) => return Err("racing move unexpectedly succeeded".into()),
    }
    assert_eq!(record(&store, &second.match_id)?.events.len(), 2);
    assert!(record(&store, &second.match_id)?.commands.is_empty());
    Ok(())
}

#[test]
fn custom_positions_are_validated_and_restart_turn_numbering() -> TestResult {
    let (service, store) = fixture()?;
    for start in [
        Start::State {
            state: json!([2, 7, 3]),
        },
        Start::Position {
            position: "[2,7,3]".into(),
        },
    ] {
        let mut request = create("counter");
        request.start = Some(start);
        let state = run(service.create_match(request, Viewer::Player(0)))?;
        assert_eq!(state.turn, 0);
        assert_eq!(state.observation.json["turn"], 2);
        assert_eq!(
            run(service.get_state(&state.match_id, Viewer::Player(0)))?,
            state
        );
        run(service.make_move(&state.match_id, action(0, json!("1")), None))?;
    }
    for state in [json!([9, 7, 0]), json!([4, 7, 5])] {
        let mut request = create("counter");
        request.start = Some(Start::State { state });
        code(
            run(service.create_match(request, Viewer::Player(0))),
            "INVALID_POSITION",
        );
    }
    let mut invalid_config = create("counter");
    invalid_config.config = json!({"unsupported":true});
    invalid_config.start = Some(Start::State {
        state: json!([0, 7, 0]),
    });
    code(
        run(service.create_match(invalid_config, Viewer::Player(0))),
        "INVALID_CONFIG",
    );
    assert_eq!(store.records.lock().map_err(|_| "poisoned")?.len(), 2);
    Ok(())
}

#[test]
fn seed_is_recorded_and_views_never_include_private_events() -> TestResult {
    let (service, store) = fixture()?;
    let mut request = create("counter");
    request.seed = None;
    let initial = run(service.create_match(request, Viewer::Spectator))?;
    assert!(initial.legal_actions.is_empty());
    assert!(initial.action_mask.is_empty());
    assert!(initial.observation.json["private"].is_null());
    let stored = record(&store, &initial.match_id)?;
    match &stored.events[0] {
        MatchEvent::MatchCreated(origin) => {
            assert_eq!(origin.seed, 42);
            assert_eq!(origin.created_at_ms, 1234);
        }
        _ => return Err("missing origin".into()),
    }
    run(service.make_move(&initial.match_id, action(0, json!("1")), None))?;
    let public = run(service.get_replay(&initial.match_id, Viewer::Spectator))?;
    assert!(
        public
            .states
            .iter()
            .all(|state| state.observation.json["private"].is_null()
                && state.legal_actions.is_empty())
    );
    let private = run(service.get_replay(&initial.match_id, Viewer::Player(1)))?;
    assert!(private
        .states
        .iter()
        .all(|state| state.observation.json["private"] == "seat-1"));
    Ok(())
}

#[test]
fn length_cap_records_a_replayable_truncation() -> TestResult {
    let (service, store) = fixture()?;
    let initial = run(service.create_match(create("capped"), Viewer::Player(0)))?;
    for turn in 0..4 {
        run(service.make_move(&initial.match_id, action(turn, json!("1")), None))?;
    }
    let state = run(service.get_state(&initial.match_id, Viewer::Player(0)))?;
    assert!(!state.terminated);
    assert!(state.truncated);
    assert!(state.to_act.is_empty());
    assert!(state.legal_actions.is_empty());
    assert_eq!(record(&store, &initial.match_id)?.events.len(), 6);
    code(
        run(service.make_move(&initial.match_id, action(4, json!("1")), None)),
        "MATCH_FINISHED",
    );
    Ok(())
}

#[test]
fn corrupt_events_and_unavailable_versions_are_rejected() -> TestResult {
    let (service, store) = fixture()?;
    let initial = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let id = &initial.match_id;
    for turn in 0..4 {
        run(service.make_move(id, action(turn, json!("1")), None))?;
    }
    let original = record(&store, id)?;
    for kind in 0..7 {
        let mut corrupted = original.clone();
        let expected = match kind {
            0 => {
                if let MatchEvent::MatchCreated(origin) = &mut corrupted.events[0] {
                    origin.engine_version = "missing".into();
                }
                "ENGINE_UNAVAILABLE"
            }
            1 => {
                if let MatchEvent::Action(action) = &mut corrupted.events[1] {
                    action.turn = 99;
                }
                "INVALID_EVENT_LOG"
            }
            2 => {
                if let MatchEvent::Action(action) = &mut corrupted.events[1] {
                    action.action.string = "2".into();
                }
                "INVALID_EVENT_LOG"
            }
            3 => {
                if let MatchEvent::Action(action) = &mut corrupted.events[1] {
                    action.engine_events.events.clear();
                }
                "INVALID_EVENT_LOG"
            }
            4 => {
                corrupted.events.pop();
                "INVALID_EVENT_LOG"
            }
            5 => {
                if let Some(MatchEvent::MatchFinished { returns, .. }) = corrupted.events.last_mut()
                {
                    *returns = vec![0.0, 0.0];
                }
                "INVALID_EVENT_LOG"
            }
            _ => {
                corrupted.events.swap(0, 1);
                "INVALID_EVENT_LOG"
            }
        };
        store
            .records
            .lock()
            .map_err(|_| "poisoned")?
            .insert(id.clone(), corrupted);
        code(run(service.get_replay(id, Viewer::Spectator)), expected);
    }
    code(
        run(service.get_state("missing", Viewer::Spectator)),
        "MATCH_NOT_FOUND",
    );
    Ok(())
}

#[test]
fn observer_runs_only_after_successful_new_commits() -> TestResult {
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);
    impl MatchObserver for Recorder {
        fn committed(&self, id: &str) {
            if let Ok(mut records) = self.0.lock() {
                records.push(id.into());
            }
        }
    }
    let recorder = Arc::new(Recorder::default());
    let (service, store) = fixture()?;
    let service = service.with_observer(recorder.clone());
    let initial = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let id = &initial.match_id;
    store.append_mode.store(1, Ordering::SeqCst);
    code(
        run(service.make_move(id, action(0, json!("1")), Some("once"))),
        "STORAGE_UNAVAILABLE",
    );
    assert_eq!(recorder.0.lock().map_err(|_| "poisoned")?.len(), 1);
    let response = run(service.make_move(id, action(0, json!("1")), Some("once")))?;
    assert_eq!(
        run(service.make_move(id, action(0, json!("1")), Some("once")))?,
        response
    );
    assert_eq!(recorder.0.lock().map_err(|_| "poisoned")?.len(), 2);
    Ok(())
}

#[test]
fn match_briefings_preserve_private_views_and_do_not_change_storage() -> TestResult {
    use gfa_api_types::InfoDetail;
    let (service, store) = fixture()?;
    let mut request = create("counter");
    request.start = Some(Start::State {
        state: json!([0, 1234567890123456_u64, 0]),
    });
    let created = run(service.create_match_with_info(request, Viewer::Player(1)))?;
    let id = &created.state.match_id;
    let original = record(&store, id)?;
    for (viewer, private) in [
        (Viewer::Player(0), json!("seat-0")),
        (Viewer::Player(1), json!("seat-1")),
        (Viewer::Spectator, Value::Null),
    ] {
        let info = run(service.get_match_info(id, viewer, InfoDetail::Full))?;
        let live = &info.sections[0];
        assert_eq!(live.id, "match");
        assert_eq!(live.data["start"]["view"]["json"]["private"], private);
        assert!(live.data["start"].get("position").is_none());
        assert_eq!(
            live.data["state"]["observation"]["json"]["private"],
            private
        );
        assert!(live.data.get("seed").is_none());
        assert!(!serde_json::to_string(&info)?.contains("1234567890123456"));
        for section in &info.sections {
            if section.id == "initial_state" {
                assert_eq!(section.data["observation"]["json"]["private"], private);
            }
        }
        if viewer == Viewer::Spectator {
            assert!(live.data["you"].is_null());
            assert_eq!(live.data["state"]["legal_actions"], json!([]));
        }
    }
    assert_eq!(record(&store, id)?, original);
    let expected = run(service.get_match_info(id, Viewer::Player(1), InfoDetail::Compact))?;
    assert_eq!(created.info, Some(expected));
    code(
        run(service.get_match_info(id, Viewer::Player(255), InfoDetail::Full)),
        "INVALID_POSITION",
    );
    for turn in 0..4 {
        run(service.make_move(id, action(turn, json!("1")), None))?;
    }
    let ended = run(service.get_match_info(id, Viewer::Player(1), InfoDetail::Compact))?;
    assert_eq!(ended.sections[0].data["status"], "finished");
    assert_eq!(ended.sections[0].data["turn"], 4);
    Ok(())
}

#[test]
fn briefing_failure_prevents_creation_and_opt_out_keeps_state_only_access() -> TestResult {
    let (service, store) = fixture()?;
    code(
        run(service.create_match_with_info(create("capped"), Viewer::Player(0))),
        "INVALID_BRIEFING",
    );
    assert!(store.records.lock().map_err(|_| "poisoned")?.is_empty());
    let mut request = create("capped");
    request.include_info = false;
    let created = run(service.create_match_with_info(request, Viewer::Player(0)))?;
    assert!(created.info.is_none());
    assert_eq!(store.records.lock().map_err(|_| "poisoned")?.len(), 1);
    Ok(())
}

#[test]
fn imported_positions_get_fresh_rng_without_storage_writes() -> TestResult {
    use gfa_api_types::ValidatePosition;
    let (service, store) = fixture()?;
    let input = json!([2, 999, 3]);
    let request = ValidatePosition {
        config: json!({}),
        start: Start::State {
            state: input.clone(),
        },
        seed: Some(17),
    };
    let imported = service.validate_position("counter", request.clone(), Viewer::Player(0))?;
    assert_eq!(imported.state, json!([2, 17, 3]));
    assert_eq!(
        service.validate_position("counter", request, Viewer::Player(0))?,
        imported
    );
    let notation = ValidatePosition {
        config: json!({}),
        start: Start::Position {
            position: imported.position,
        },
        seed: None,
    };
    let fresh = service.validate_position("counter", notation, Viewer::Player(0))?;
    assert_eq!(fresh.seed, 42);
    assert_eq!(fresh.state, json!([2, 42, 3]));
    assert_eq!(fresh.observation, imported.observation);
    assert_eq!(input, json!([2, 999, 3]));
    assert!(store.records.lock().map_err(|_| "poisoned")?.is_empty());
    let created = run(service.create_match(
        CreateMatch {
            game_id: "counter".into(),
            config: json!({}),
            seed: Some(17),
            start: Some(Start::State { state: input }),
            include_info: false,
            seats: vec![],
            assists: gfa_api_types::Assists::default(),
        },
        Viewer::Player(0),
    ))?;
    assert_eq!(created.observation, imported.observation);
    let replay = run(service.get_replay(&created.match_id, Viewer::Player(0)))?;
    assert_eq!(replay.states[0], created);
    Ok(())
}

#[test]
fn control_events_preserve_hidden_boards_and_reject_tampered_logs() -> TestResult {
    use gfa_api_types::ControlRequest;
    let (service, store) = fixture()?;
    let initial = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let id = &initial.match_id;
    let resigned = run(service.resign(id, ControlRequest { seat: 0, turn: 0 }))?;
    assert_eq!(resigned.observation, initial.observation);
    assert_eq!(resigned.returns, vec![-1.0, 1.0]);
    assert_eq!(
        run(service.get_replay(id, Viewer::Player(0)))?.states,
        vec![resigned]
    );
    let mut changed = record(&store, id)?;
    match changed.events.last_mut() {
        Some(MatchEvent::Resigned { turn, .. }) => *turn = 1,
        _ => return Err("missing resignation".into()),
    }
    store
        .records
        .lock()
        .map_err(|_| "poisoned")?
        .insert(id.clone(), changed);
    code(
        run(service.get_state(id, Viewer::Player(0))),
        "INVALID_EVENT_LOG",
    );
    Ok(())
}

#[test]
fn simulation_reseeds_hidden_chance_without_touching_the_live_record() -> TestResult {
    use gfa_api_types::{SimulateRequest, SimulationFrom, SimulationOutput};
    let (service, store) = fixture()?;
    let initial = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let id = &initial.match_id;
    let before = record(&store, id)?;
    let request = SimulateRequest {
        from: SimulationFrom::Match {
            match_id: id.clone(),
            seat: 0,
            turn: None,
        },
        config: json!({}),
        seed: Some(71),
        output: SimulationOutput::All,
        lines: vec![vec![json!("1"), json!("2")], vec![json!("1"), json!("2")]],
    };
    let simulated = run(service.simulate("counter", request.clone(), Viewer::Player(0)))?;
    assert_eq!(
        serde_json::to_value(&simulated.lines[0])?,
        serde_json::to_value(&simulated.lines[1])?
    );
    assert_eq!(
        serde_json::to_value(&simulated)?,
        serde_json::to_value(run(service.simulate(
            "counter",
            request,
            Viewer::Player(0)
        ))?)?
    );
    assert_eq!(record(&store, id)?, before);
    let clone = run(service.create_match(
        CreateMatch {
            seed: Some(71),
            ..create("counter")
        },
        Viewer::Player(0),
    ))?;
    for (turn, action_value) in [json!("1"), json!("2")].into_iter().enumerate() {
        run(service.make_move(&clone.match_id, action(turn as u64, action_value), None))?;
        let mut played = run(service.get_state(&clone.match_id, Viewer::Player(0)))?;
        played.match_id.clear();
        assert_eq!(simulated.lines[0].states[turn], played);
        assert_eq!(played.observation.json["private"], "seat-0");
    }
    let usage = run(store.assist_usage(id))?;
    assert_eq!(usage[0].simulation_calls, 2);
    assert_eq!(usage[0].simulated_moves, 8);
    Ok(())
}

#[test]
fn perfect_information_does_not_opt_in_to_raw_position_disclosure() -> TestResult {
    let mut registry = GameRegistry::default();
    registry.register::<Counter<true, true>>()?;
    let host = Arc::new(Host::default());
    let service = GameService::new(
        registry,
        Arc::new(MemoryStore::default()),
        host.clone(),
        host,
    );
    let created = run(service.create_match_with_info(
        CreateMatch {
            start: Some(Start::State {
                state: json!([1, 7, 2]),
            }),
            ..create("counter")
        },
        Viewer::Player(0),
    ))?;
    let info = created.info.ok_or("briefing")?;
    let section = info
        .sections
        .iter()
        .find(|section| section.id == "match")
        .ok_or("match")?;
    assert!(section.data["start"].get("position").is_none());
    assert_eq!(
        section.data["start"]["view"],
        serde_json::to_value(&created.state.observation)?
    );
    let admin = run(service.get_match_info(
        &created.state.match_id,
        Viewer::Omniscient,
        gfa_api_types::InfoDetail::Compact,
    ))?;
    assert!(admin.sections[0].data["start"]["position"].is_string());
    Ok(())
}

fn fork_request(turn: u64, keep_rng: bool) -> gfa_api_types::ForkMatch {
    gfa_api_types::ForkMatch {
        seats: None,
        turn,
        keep_rng,
        seed: None,
        include_info: false,
    }
}

#[test]
fn forks_enforce_ownership_benchmark_and_hidden_rng_permissions() -> TestResult {
    let (service, store) = fixture()?;
    let root = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let access = ForkAccess {
        viewer: Viewer::Player(0),
        owns_parent: false,
        full_state: false,
    };
    code(
        run(service.fork_match(&root.match_id, fork_request(0, false), access)),
        "FORBIDDEN",
    );
    let owner = ForkAccess {
        owns_parent: true,
        ..access
    };
    code(
        run(service.fork_match(&root.match_id, fork_request(0, true), owner)),
        "FORBIDDEN",
    );
    code(
        run(service.fork_match(
            &root.match_id,
            fork_request(0, false),
            ForkAccess {
                viewer: Viewer::Omniscient,
                ..owner
            },
        )),
        "FORBIDDEN",
    );
    {
        let mut records = store.records.lock().map_err(|_| "poisoned")?;
        let record = records.get_mut(&root.match_id).ok_or("record")?;
        if let MatchEvent::MatchCreated(origin) = &mut record.events[0] {
            origin.benchmark_run = Some("run-1".into());
        }
    }
    code(
        run(service.fork_match(&root.match_id, fork_request(0, false), owner)),
        "FORBIDDEN",
    );
    run(service.resign(
        &root.match_id,
        gfa_api_types::ControlRequest { seat: 0, turn: 0 },
    ))?;
    let variation = run(service.fork_match(&root.match_id, fork_request(0, false), access))?;
    assert!(!variation.state.terminated);
    assert!(record(&store, &variation.state.match_id)?
        .origin()
        .ok_or("origin")?
        .benchmark_run
        .is_none());
    Ok(())
}

#[test]
fn forks_resample_observations_or_preserve_serialized_chance_exactly() -> TestResult {
    let (service, store) = fixture()?;
    let root = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    run(service.make_move(&root.match_id, action(0, json!("1")), None))?;
    let before = record(&store, &root.match_id)?;
    let owner = ForkAccess {
        viewer: Viewer::Player(0),
        owns_parent: true,
        full_state: false,
    };
    let fresh = run(service.fork_match(
        &root.match_id,
        gfa_api_types::ForkMatch {
            seed: Some(991),
            ..fork_request(1, false)
        },
        owner,
    ))?;
    let keep = run(service.fork_match(
        &root.match_id,
        fork_request(1, true),
        ForkAccess {
            full_state: true,
            ..owner
        },
    ))?;
    assert_eq!(record(&store, &root.match_id)?, before);
    let source = replay::reconstruct(&service.registry, &before)?
        .current()?
        .state
        .clone();
    let fresh_record = record(&store, &fresh.state.match_id)?;
    let fresh_origin = fresh_record.origin().ok_or("origin")?;
    assert_eq!(fresh_origin.seed, 991);
    assert!(!fresh_origin.preserve_start_rng);
    assert_eq!(
        replay::reconstruct(&service.registry, &fresh_record)?
            .current()?
            .state[1],
        991
    );
    assert_eq!(
        replay::reconstruct(&service.registry, &record(&store, &keep.state.match_id)?)?
            .current()?
            .state,
        source
    );
    let root_next = run(service.make_move(&root.match_id, action(1, json!("2")), None))?;
    let keep_next = run(service.make_move(
        &keep.state.match_id,
        MoveRequest {
            seat: 1,
            turn: 0,
            action: json!("2"),
            reasoning: None,
        },
        None,
    ))?;
    assert_eq!(root_next.state.observation, keep_next.state.observation);
    let lineage = run(service.get_replay(&keep.state.match_id, Viewer::Player(0)))?;
    assert_eq!(lineage.ancestors.len(), 1);
    assert!(lineage.ancestors[0]
        .states
        .iter()
        .all(|state| state.observation.json["private"].is_null()));
    // Corrupt cyclic references are rejected rather than recursing indefinitely.
    {
        let mut records = store.records.lock().map_err(|_| "poisoned")?;
        let child = records.get_mut(&keep.state.match_id).ok_or("child")?;
        if let MatchEvent::ForkedFrom { source, .. } = &mut child.events[0] {
            source.match_id = keep.state.match_id.clone();
        }
    }
    assert!(run(service.get_replay(&keep.state.match_id, Viewer::Player(0))).is_err());
    Ok(())
}

#[test]
fn fork_depth_is_bounded_without_changing_existing_matches() -> TestResult {
    let (service, store) = fixture()?;
    let root = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    let access = ForkAccess {
        viewer: Viewer::Player(0),
        owns_parent: true,
        full_state: false,
    };
    let mut id = root.match_id;
    for _ in 0..32 {
        id = run(service.fork_match(&id, fork_request(0, false), access))?
            .state
            .match_id;
    }
    let before = store.records.lock().map_err(|_| "poisoned")?.len();
    code(
        run(service.fork_match(&id, fork_request(0, false), access)),
        "INVALID_CONFIG",
    );
    assert_eq!(store.records.lock().map_err(|_| "poisoned")?.len(), before);
    Ok(())
}

struct ImmediateOpponentExecutor;
impl OpponentExecutor for ImmediateOpponentExecutor {
    fn execute(&self, job: OpponentJob) -> OpponentFuture<'_> {
        struct FixedClock;
        impl gfa_opponents::Clock for FixedClock {
            fn now_ms(&self) -> u64 {
                0
            }
        }
        Box::pin(async move { job.run(&FixedClock) })
    }
}

struct ObservingFactory;
impl OpponentFactory for ObservingFactory {
    fn catalog(&self, game: &dyn gfa_core::DynGame) -> Vec<gfa_api_types::OpponentSpec> {
        BuiltinOpponentFactory.catalog(game)
    }
    fn create(
        &self,
        _: Arc<dyn gfa_core::DynGame>,
        _: Value,
        _: &gfa_api_types::OpponentConfig,
        seed: u64,
    ) -> Result<
        (
            Arc<dyn gfa_opponents::Opponent>,
            gfa_opponents::SearchLimits,
        ),
        ApiError,
    > {
        struct ObservingPlayer;
        impl gfa_opponents::Opponent for ObservingPlayer {
            fn choose_action(
                &self,
                turn: &gfa_opponents::PlayerTurn<'_>,
                limits: gfa_opponents::SearchLimits,
                clock: &dyn gfa_opponents::Clock,
            ) -> Result<gfa_opponents::ActionChoice, GameError> {
                assert_eq!(turn.observation.json["private"], "seat-0");
                assert!(turn.observation.json.get("seed").is_none());
                gfa_opponents::Random.choose_action(turn, limits, clock)
            }
        }
        Ok((
            Arc::new(ObservingPlayer),
            gfa_opponents::SearchLimits::for_level(1, seed).map_err(error::engine)?,
        ))
    }
}

#[test]
fn analysis_uses_only_the_authorized_observation_and_leaves_the_record_unchanged() -> TestResult {
    let (service, store) = fixture()?;
    assert!(service.list_opponents("counter")?.is_empty());
    let service = service.with_opponents(
        Arc::new(ObservingFactory),
        Arc::new(ImmediateOpponentExecutor),
    );
    let mut options = create("counter");
    options.assists.allow_analysis = true;
    let initial = run(service.create_match(options, Viewer::Player(0)))?;
    let before = serde_json::to_value(record(&store, &initial.match_id)?)?;
    let request: gfa_api_types::AnalysisRequest = serde_json::from_value(json!({
        "game_id":"counter", "from":{"match_id":initial.match_id,"seat":0},
        "opponent":{"id":"random"}, "seed":19
    }))?;
    let first = run(service.analyze(request.clone(), Viewer::Player(0)))?;
    assert!(initial.legal_actions.contains(&first.best_moves[0]));
    assert_eq!(
        first,
        run(service.analyze(request.clone(), Viewer::Player(0)))?
    );
    assert_eq!(
        before,
        serde_json::to_value(record(&store, &initial.match_id)?)?
    );
    assert!(run(store.assist_usage(&initial.match_id))?.is_empty());
    code(
        run(service.analyze(request.clone(), Viewer::Player(1))),
        "ASSIST_NOT_ALLOWED",
    );
    code(
        run(service.analyze(request, Viewer::Spectator)),
        "FORBIDDEN",
    );
    Ok(())
}

#[test]
fn analysis_assists_reject_match_and_standalone_bypasses_and_unsupported_search() -> TestResult {
    let (service, _) = fixture()?;
    let service = service.with_opponents(
        Arc::new(BuiltinOpponentFactory),
        Arc::new(ImmediateOpponentExecutor),
    );
    assert_eq!(
        service
            .list_opponents("counter")?
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        vec!["random"]
    );
    let initial = run(service.create_match(create("counter"), Viewer::Player(0)))?;
    for from in [
        json!({"match_id":initial.match_id,"seat":0}),
        json!({"state":[0,42,0]}),
        json!({"position":"[0,42,0]"}),
    ] {
        let request = serde_json::from_value(
            json!({"game_id":"counter","from":from,"opponent":{"id":"random"},"seed":7}),
        )?;
        code(
            run(service.analyze(request, Viewer::Player(0))),
            "ASSIST_NOT_ALLOWED",
        );
    }
    run(service.resign(
        &initial.match_id,
        gfa_api_types::ControlRequest { seat: 0, turn: 0 },
    ))?;
    let mut request: gfa_api_types::AnalysisRequest = serde_json::from_value(json!({
        "game_id":"counter","from":{"state":[0,42,0]},"opponent":{"id":"minimax"},"seed":7
    }))?;
    code(
        run(service.analyze(request.clone(), Viewer::Player(0))),
        "OPPONENT_UNAVAILABLE",
    );
    request.opponent.id = "random".into();
    request.opponent.limits.nodes = Some(0);
    code(
        run(service.analyze(request, Viewer::Player(0))),
        "INVALID_CONFIG",
    );
    Ok(())
}

fn opponent_seat() -> gfa_api_types::Seat {
    gfa_api_types::Seat::Opponent {
        opponent: gfa_api_types::OpponentConfig {
            id: "random".into(),
            level: None,
            limits: Default::default(),
        },
        seed: Some(12345),
    }
}

struct FailingOpponentExecutor;
impl OpponentExecutor for FailingOpponentExecutor {
    fn execute(&self, _: OpponentJob) -> OpponentFuture<'_> {
        Box::pin(async {
            Err(ApiError::new(
                "ENGINE_UNAVAILABLE",
                "Injected worker failure",
                "Retry.",
            ))
        })
    }
}

#[test]
fn automatic_replies_are_atomic_idempotent_and_do_not_leak_the_other_seat() -> TestResult {
    let (service, store) = fixture()?;
    let service = service.with_opponents(
        Arc::new(BuiltinOpponentFactory),
        Arc::new(ImmediateOpponentExecutor),
    );
    let mut options = create("counter");
    options.seats = vec![gfa_api_types::Seat::SelfPlayer, opponent_seat()];
    let initial = run(service.create_match(options, Viewer::Player(0)))?;
    let request = action(0, json!(1));
    let first = run(service.make_move(&initial.match_id, request.clone(), Some("turn-zero")))?;
    assert_eq!(first.state.turn, 2);
    assert_eq!(first.state.to_act, [0]);
    assert_eq!(first.state.observation.json["private"], "seat-0");
    assert_eq!(first.opponent_actions.len(), 1);
    assert_eq!(first.opponent_actions[0].seat, 1);
    assert!(first.opponent_actions[0].action.is_none());
    assert_eq!(
        first,
        run(service.make_move(&initial.match_id, request.clone(), Some("turn-zero")))?
    );
    let saved = record(&store, &initial.match_id)?;
    assert_eq!(saved.commands.len(), 1);
    assert!(saved.events.iter().any(|event| matches!(
        event,
        MatchEvent::Action(AppliedAction {
            seat: 1,
            opponent_info: Some(_),
            ..
        })
    )));
    code(
        run(service.make_move(&initial.match_id, action(1, json!(2)), None)),
        "FORBIDDEN",
    );
    let finished = run(service.make_move(&initial.match_id, action(2, json!(1)), None))?;
    assert!(finished.state.terminated);
    assert_eq!(finished.state.turn, 4);
    assert_eq!(
        first,
        run(service.make_move(&initial.match_id, request, Some("turn-zero")))?
    );
    let replay = run(service.get_replay(&initial.match_id, Viewer::Player(0)))?;
    assert_eq!(replay.states.len(), 5);
    assert_eq!(replay.states[4], finished.state);
    Ok(())
}

#[test]
fn worker_failure_rolls_back_the_callers_move_and_opponent_openings() -> TestResult {
    let (service, store) = fixture()?;
    let service = service.with_opponents(
        Arc::new(BuiltinOpponentFactory),
        Arc::new(FailingOpponentExecutor),
    );
    let mut options = create("counter");
    options.seats = vec![gfa_api_types::Seat::SelfPlayer, opponent_seat()];
    let initial = run(service.create_match(options.clone(), Viewer::Player(0)))?;
    let before = record(&store, &initial.match_id)?;
    code(
        run(service.make_move(&initial.match_id, action(0, json!(1)), Some("retry"))),
        "ENGINE_UNAVAILABLE",
    );
    assert_eq!(record(&store, &initial.match_id)?, before);
    options.seats.reverse();
    code(
        run(service.create_match(options, Viewer::Player(1))),
        "ENGINE_UNAVAILABLE",
    );
    assert_eq!(run(store.list_ids("", 100))?, vec![initial.match_id]);
    Ok(())
}

#[test]
fn invalid_or_unconfigured_opponent_seats_fail_before_persistence() -> TestResult {
    let (service, store) = fixture()?;
    let mut options = create("counter");
    options.seats = vec![gfa_api_types::Seat::SelfPlayer, opponent_seat()];
    code(
        run(service.create_match(options.clone(), Viewer::Player(0))),
        "ENGINE_UNAVAILABLE",
    );
    let service = service.with_opponents(
        Arc::new(BuiltinOpponentFactory),
        Arc::new(ImmediateOpponentExecutor),
    );
    options.seats = vec![opponent_seat()];
    code(
        run(service.create_match(options.clone(), Viewer::Player(0))),
        "INVALID_CONFIG",
    );
    options.seats = vec![opponent_seat(), opponent_seat()];
    code(
        run(service.create_match(options, Viewer::Player(0))),
        "INVALID_CONFIG",
    );
    assert!(run(store.list_ids("", 100))?.is_empty());
    Ok(())
}

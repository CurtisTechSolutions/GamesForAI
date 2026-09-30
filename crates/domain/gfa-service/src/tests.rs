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
struct Counter<const TERMINATES: bool>;

impl<const TERMINATES: bool> Game for Counter<TERMINATES> {
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
            information: Information::Imperfect,
            stochastic: true,
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
            observation: "Public turn and total; only a player's own private label is visible.".into(),
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

#[derive(Default)]
struct MemoryStore {
    records: Mutex<BTreeMap<String, MatchRecord>>,
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
    request.start = Some(Start::State { state:json!([0,1234567890123456_u64,0]) });
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
        assert_eq!(live.id,"match");
        assert_eq!(live.data["start"]["view"]["json"]["private"],private);
        assert!(live.data["start"].get("position").is_none());
        assert_eq!(live.data["state"]["observation"]["json"]["private"],private);
        assert!(live.data.get("seed").is_none());
        assert!(!serde_json::to_string(&info)?.contains("1234567890123456"));
        for section in &info.sections {
            if section.id == "initial_state" {
                assert_eq!(section.data["observation"]["json"]["private"],private);
            }
        }
        if viewer == Viewer::Spectator {
            assert!(live.data["you"].is_null());
            assert_eq!(live.data["state"]["legal_actions"], json!([]));
        }
    }
    assert_eq!(record(&store,id)?, original);
    let expected = run(service.get_match_info(id,Viewer::Player(1),InfoDetail::Compact))?;
    assert_eq!(created.info,Some(expected));
    code(run(service.get_match_info(id,Viewer::Player(255),InfoDetail::Full)),"INVALID_POSITION");
    for turn in 0..4 { run(service.make_move(id,action(turn,json!("1")),None))?; }
    let ended = run(service.get_match_info(id,Viewer::Player(1),InfoDetail::Compact))?;
    assert_eq!(ended.sections[0].data["status"],"finished");
    assert_eq!(ended.sections[0].data["turn"],4);
    Ok(())
}

#[test]
fn briefing_failure_prevents_creation_and_opt_out_keeps_state_only_access() -> TestResult {
    let (service, store) = fixture()?;
    code(run(service.create_match_with_info(create("capped"),Viewer::Player(0))),"INVALID_BRIEFING");
    assert!(store.records.lock().map_err(|_|"poisoned")?.is_empty());
    let mut request = create("capped");
    request.include_info = false;
    let created = run(service.create_match_with_info(request,Viewer::Player(0)))?;
    assert!(created.info.is_none());
    assert_eq!(store.records.lock().map_err(|_|"poisoned")?.len(),1);
    Ok(())
}

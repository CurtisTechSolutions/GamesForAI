use super::*;
use gfa_api_types::{MatchState, MoveRequest, MoveResult};
use gfa_core::{LegalAction, Observation, StepEvents};
use gfa_service::{AppliedAction, MatchOrigin};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn new_record(id: &str) -> MatchRecord {
    MatchRecord {
        id: id.into(),
        events: vec![MatchEvent::MatchCreated(MatchOrigin {
            game_id: "fixture".into(),
            engine_version: "1.0.0".into(),
            config: json!({}),
            seed: 42,
            start: None,
            created_at_ms: 1234,
            preserve_start_rng: false,
            benchmark_run: None,
            assists: gfa_api_types::Assists::default(),
        })],
        commands: vec![],
    }
}

fn command(key: &str) -> StoredCommand {
    StoredCommand {
        key: key.into(),
        request: MoveRequest {
            seat: 0,
            turn: 0,
            action: json!("move"),
            reasoning: None,
        },
        response: MoveResult {
            accepted_action: LegalAction {
                string: "move".into(),
                json: json!({"cell":0}),
                index: 0,
            },
            state: MatchState {
                match_id: "fixture".into(),
                turn: 1,
                to_act: vec![1],
                observation: Observation {
                    text: "board".into(),
                    json: json!({}),
                    tensor: None,
                },
                legal_actions: vec![],
                action_mask: vec![false],
                returns: vec![0.0, 0.0],
                terminated: false,
                truncated: false,
                outcome: None,
                draw_offer: None,
            },
        },
    }
}

fn events() -> Vec<MatchEvent> {
    vec![MatchEvent::Action(AppliedAction {
        turn: 0,
        seat: 0,
        action: command("unused").response.accepted_action,
        reasoning: Some("fixture".into()),
        engine_events: StepEvents::default(),
        accepted_at_ms: 1235,
    })]
}

async fn get(store: &PostgresMatchStore, id: &str) -> Result<MatchRecord, StoreError> {
    store.load(id).await?.ok_or(StoreError::NotFound)
}

/// The dedicated PostgreSQL CI job supplies an isolated database.
#[tokio::test]
#[ignore = "requires GFA_TEST_POSTGRES_URL pointing at an isolated empty database"]
async fn postgres_atomicity_concurrency_snapshots_and_restart() -> TestResult {
    let url = std::env::var("GFA_TEST_POSTGRES_URL")?;
    let store = PostgresMatchStore::connect(&url).await?;
    let id = "match'; DROP TABLE gfa_matches; --";
    let original = new_record(id);
    store.create(original.clone()).await?;
    assert_eq!(get(&store, id).await?, original);
    assert_eq!(store.create(original).await, Err(StoreError::DuplicateId));
    store
        .append(id, 1, events(), Some(command("key-α")))
        .await?;
    let expected = get(&store, id).await?;
    assert_eq!(
        store
            .append(id, 0, events(), Some(command("key-α")))
            .await?,
        AppendResult::AlreadyCommitted(Box::new(command("key-α").response))
    );
    let mut changed = command("key-α");
    changed.request.action = json!("different");
    assert_eq!(
        store.append(id, 2, events(), Some(changed)).await,
        Err(StoreError::IdempotencyConflict)
    );
    assert_eq!(get(&store, id).await?, expected);

    store.create(new_record("race")).await?;
    let (left, right) = tokio::join!(
        store.append("race", 1, events(), None),
        store.append("race", 1, events(), None)
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(left == Err(StoreError::Conflict) || right == Err(StoreError::Conflict));
    assert_eq!(get(&store, "race").await?.events.len(), 2);

    store.create(new_record("retry")).await?;
    let (left, right) = tokio::join!(
        store.append("retry", 1, events(), Some(command("same"))),
        store.append("retry", 1, events(), Some(command("same")))
    );
    assert!(left.is_ok() && right.is_ok());
    let retried = get(&store, "retry").await?;
    assert_eq!(retried.events.len(), 2);
    assert_eq!(retried.commands.len(), 1);

    let mut broken = new_record("rollback");
    broken.commands = vec![command("duplicate"), command("duplicate")];
    assert!(store.create(broken).await.is_err());
    assert_eq!(store.load("rollback").await?, None);
    assert_eq!(
        store.append("missing", 0, events(), None).await,
        Err(StoreError::NotFound)
    );

    store.create(new_record("snapshot")).await?;
    let writer = async {
        for revision in 1..=64 {
            store.append("snapshot", revision, events(), None).await?;
        }
        Ok::<_, StoreError>(())
    };
    let reader = async {
        for _ in 0..128 {
            let snapshot = get(&store, "snapshot").await?;
            assert!((1..=65).contains(&snapshot.events.len()));
        }
        Ok::<_, StoreError>(())
    };
    let (written, read) = tokio::join!(writer, reader);
    written?;
    read?;

    let mut updates = Vec::new();
    for _ in 0..32 {
        let store = store.clone();
        updates.push(tokio::spawn(async move {
            store.record_simulation("snapshot", 0, 5).await
        }));
    }
    for update in updates {
        update.await??;
    }
    assert_eq!(
        store.assist_usage("snapshot").await?,
        vec![gfa_api_types::AssistUsage {
            seat: 0,
            simulation_calls: 32,
            simulated_moves: 160,
        }]
    );
    assert!(store.record_simulation("missing", 0, 1).await.is_err());

    for id in ["Z", "a", "aa", "a-", "ä"] {
        store.create(new_record(id)).await?;
    }
    let mut ids = Vec::new();
    let mut cursor = String::new();
    loop {
        let next = store.list_ids(&cursor, 2).await?;
        if next.is_empty() {
            break;
        }
        assert!(next.iter().all(|id| id > &cursor));
        cursor = next.last().ok_or("cursor")?.clone();
        ids.extend(next);
    }
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(store.list_ids("", 0).await.is_err());
    let before = get(&store, "snapshot").await?;
    store.close().await;
    let reopened = PostgresMatchStore::connect(&url).await?;
    assert_eq!(get(&reopened, id).await?, expected);
    assert_eq!(get(&reopened, "snapshot").await?, before);
    assert_eq!(
        reopened.assist_usage("snapshot").await?[0].simulation_calls,
        32
    );
    reopened.close().await;
    Ok(())
}

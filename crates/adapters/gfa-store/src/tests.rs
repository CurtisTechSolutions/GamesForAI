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

async fn get(store: &SqliteMatchStore, id: &str) -> Result<MatchRecord, StoreError> {
    store.load(id).await?.ok_or(StoreError::NotFound)
}

#[tokio::test]
async fn disk_records_and_receipts_survive_reopen() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("matches.sqlite");
    let store = SqliteMatchStore::open(&path).await?;
    let id = "match'; DROP TABLE gfa_matches; --";
    let original = new_record(id);
    store.create(original.clone()).await?;
    assert_eq!(get(&store, id).await?, original);
    assert_eq!(
        store.create(new_record(id)).await,
        Err(StoreError::DuplicateId)
    );
    store
        .append(id, 1, events(), Some(command("key-α")))
        .await?;
    let expected = get(&store, id).await?;
    store.close().await;
    let reopened = SqliteMatchStore::open(&path).await?;
    assert_eq!(get(&reopened, id).await?, expected);
    assert_eq!(
        reopened
            .append(id, 1, events(), Some(command("key-α")))
            .await?,
        AppendResult::AlreadyCommitted(Box::new(command("key-α").response))
    );
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn keys_are_checked_before_revision_and_scoped_to_the_match() -> TestResult {
    let store = SqliteMatchStore::in_memory().await?;
    store.create(new_record("a")).await?;
    store.create(new_record("b")).await?;
    store.append("a", 1, events(), Some(command("one"))).await?;
    let before = get(&store, "a").await?;
    assert_eq!(
        store.append("a", 1, events(), None).await,
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store.append("a", 1, events(), Some(command("one"))).await?,
        AppendResult::AlreadyCommitted(Box::new(command("one").response))
    );
    let mut changed = command("one");
    changed.request.reasoning = Some("different request".into());
    assert_eq!(
        store.append("a", 1, events(), Some(changed)).await,
        Err(StoreError::IdempotencyConflict)
    );
    assert_eq!(get(&store, "a").await?, before);
    assert_eq!(
        store.append("b", 1, events(), Some(command("one"))).await?,
        AppendResult::Appended
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn independent_pools_deduplicate_concurrent_retries() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("race.sqlite");
    let a = SqliteMatchStore::open(&path).await?;
    let b = SqliteMatchStore::open(&path).await?;
    a.create(new_record("race")).await?;
    let (left, right) = tokio::join!(
        a.append("race", 1, events(), Some(command("same"))),
        b.append("race", 1, events(), Some(command("same")))
    );
    let results = [left?, right?];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, AppendResult::Appended))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, AppendResult::AlreadyCommitted(_)))
            .count(),
        1
    );
    let record = get(&a, "race").await?;
    assert_eq!(record.events.len(), 2);
    assert_eq!(record.commands.len(), 1);
    a.close().await;
    b.close().await;
    Ok(())
}

#[tokio::test]
async fn concurrent_different_moves_have_exactly_one_winner() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("race.sqlite");
    let a = SqliteMatchStore::open(&path).await?;
    let b = SqliteMatchStore::open(&path).await?;
    a.create(new_record("race")).await?;
    let (left, right) = tokio::join!(
        a.append("race", 1, events(), Some(command("left"))),
        b.append("race", 1, events(), Some(command("right")))
    );
    let results = [left, right];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Ok(AppendResult::Appended)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(StoreError::Conflict)))
            .count(),
        1
    );
    let record = get(&a, "race").await?;
    assert_eq!(record.events.len(), 2);
    assert_eq!(record.commands.len(), 1);
    a.close().await;
    b.close().await;
    Ok(())
}

#[tokio::test]
async fn failed_batch_and_failed_commit_update_roll_back_every_write() -> TestResult {
    let store = SqliteMatchStore::in_memory().await?;
    store.create(new_record("atomic")).await?;
    let before = get(&store, "atomic").await?;
    sqlx::raw_sql("CREATE TRIGGER fail_event BEFORE INSERT ON gfa_events WHEN NEW.sequence = 2 BEGIN SELECT RAISE(ABORT, 'injected'); END;")
        .execute(&store.pool).await?;
    let mut batch = events();
    batch.extend(events());
    assert!(matches!(
        store
            .append("atomic", 1, batch, Some(command("batch")))
            .await,
        Err(StoreError::Unavailable(_))
    ));
    assert_eq!(get(&store, "atomic").await?, before);
    sqlx::raw_sql("DROP TRIGGER fail_event; CREATE TRIGGER fail_revision BEFORE UPDATE ON gfa_matches BEGIN SELECT RAISE(ABORT, 'injected'); END;")
        .execute(&store.pool).await?;
    assert!(matches!(
        store
            .append("atomic", 1, events(), Some(command("receipt")))
            .await,
        Err(StoreError::Unavailable(_))
    ));
    assert_eq!(get(&store, "atomic").await?, before);
    sqlx::raw_sql("DROP TRIGGER fail_revision;")
        .execute(&store.pool)
        .await?;
    assert_eq!(
        store
            .append("atomic", 1, events(), Some(command("receipt")))
            .await?,
        AppendResult::Appended
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn invalid_import_rolls_back_match_events_and_receipts() -> TestResult {
    let store = SqliteMatchStore::in_memory().await?;
    let mut record = new_record("invalid");
    record.commands = vec![command("duplicate"), command("duplicate")];
    assert!(matches!(
        store.create(record).await,
        Err(StoreError::Unavailable(_))
    ));
    assert!(store.load("invalid").await?.is_none());
    store.create(new_record("invalid")).await?;
    assert_eq!(get(&store, "invalid").await?, new_record("invalid"));
    assert_eq!(
        store.append("missing", 0, events(), None).await,
        Err(StoreError::NotFound)
    );
    store.close().await;
    assert!(matches!(
        store.load("invalid").await,
        Err(StoreError::Unavailable(_))
    ));
    Ok(())
}

#[tokio::test]
async fn corrupt_event_sequences_fail_instead_of_returning_partial_history() -> TestResult {
    let store = SqliteMatchStore::in_memory().await?;
    store.create(new_record("broken")).await?;
    store.append("broken", 1, events(), None).await?;
    sqlx::query("DELETE FROM gfa_events WHERE match_id = ? AND sequence = 0")
        .bind("broken")
        .execute(&store.pool)
        .await?;
    assert!(matches!(
        store.load("broken").await,
        Err(StoreError::Unavailable(_))
    ));
    // Keep the count consistent to test sequence validation separately.
    sqlx::query("UPDATE gfa_matches SET revision = 1 WHERE id = ?")
        .bind("broken")
        .execute(&store.pool)
        .await?;
    assert!(matches!(
        store.load("broken").await,
        Err(StoreError::Unavailable(_))
    ));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn history_cursor_is_ordered_bounded_and_parameterized() -> TestResult {
    let store = SqliteMatchStore::in_memory().await?;
    for id in ["c", "a", "b"] {
        store.create(new_record(id)).await?;
    }
    assert_eq!(store.list_ids("", 2).await?, vec!["a", "b"]);
    assert_eq!(store.list_ids("b", 2).await?, vec!["c"]);
    assert!(store.list_ids("c", 2).await?.is_empty());
    assert_eq!(store.list_ids("' OR 1=1 --", 101).await?.len(), 3);
    assert!(store.list_ids("", 0).await.is_err());
    assert!(store.list_ids("", 102).await.is_err());
    store.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_database_initialization_precedes_pool_expansion() -> TestResult {
    let directory = tempfile::tempdir()?;
    for iteration in 0..32 {
        let path = directory.path().join(format!("fresh-{iteration}.sqlite"));
        let store = SqliteMatchStore::open(&path).await?;
        store.create(new_record("startup")).await?;
        assert_eq!(store.load("startup").await?, Some(new_record("startup")));
        store.close().await;
        let restored = SqliteMatchStore::open(&path).await?;
        assert_eq!(restored.load("startup").await?, Some(new_record("startup")));
        restored.close().await;
    }
    let memory = SqliteMatchStore::in_memory().await?;
    memory.create(new_record("memory")).await?;
    assert_eq!(memory.load("memory").await?, Some(new_record("memory")));
    memory.close().await;
    Ok(())
}

#[tokio::test]
async fn simulation_counters_are_atomic_and_leave_events_unchanged() -> TestResult {
    let store = SqliteMatchStore::in_memory().await?;
    let record = new_record("planning");
    store.create(record.clone()).await?;
    let (a, b) = tokio::join!(
        store.record_simulation("planning", 0, 7),
        store.record_simulation("planning", 0, 3),
    );
    a?;
    b?;
    store.record_simulation("planning", 1, 0).await?;
    assert_eq!(store.load("planning").await?, Some(record));
    assert_eq!(
        store.assist_usage("planning").await?,
        vec![
            gfa_api_types::AssistUsage {
                seat: 0,
                simulation_calls: 2,
                simulated_moves: 10
            },
            gfa_api_types::AssistUsage {
                seat: 1,
                simulation_calls: 1,
                simulated_moves: 0
            },
        ]
    );
    assert!(store.record_simulation("missing", 0, 1).await.is_err());
    store.close().await;
    Ok(())
}

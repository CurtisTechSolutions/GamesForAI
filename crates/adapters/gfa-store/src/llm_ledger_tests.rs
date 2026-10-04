use gfa_service::{llm_ledger::*, MatchEvent, MatchOrigin, MatchRecord, MatchStore};
use serde_json::json;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn record(id: &str) -> MatchRecord {
    MatchRecord {
        id: id.into(),
        commands: vec![],
        events: vec![MatchEvent::MatchCreated(MatchOrigin {
            game_id: "fixture".into(),
            engine_version: "1".into(),
            config: json!({}),
            seed: 1,
            start: None,
            seats: vec![],
            assists: Default::default(),
            created_at_ms: 1,
            preserve_start_rng: false,
            benchmark_run: None,
        })],
    }
}
fn reservation(id: &str, match_id: &str, key: &str) -> Result<CallReservation, serde_json::Error> {
    serde_json::from_value(json!({
        "id":id,"match_id":match_id,"seat":0,"turn":0,"attempt":0,"created_at_ms":123,
        "request":{"config":{"mode":"direct","max_output_tokens":10},"system":"Immutable rules.","messages":[{"role":"user","content":[{"type":"text","text":"Choose a move."}]}],"tools":[]},
        "pricing":{"version":"test-only","input":1000000,"output":1000000,"cache_read":1000000,"cache_write_5m":1000000,"cache_write_1h":1000000},
        "scopes":[
            {"id":format!("match:{match_id}"),"limit":{"tokens":1000,"cost_microusd":1000}},
            {"id":format!("key:{key}"),"limit":{"tokens":150,"cost_microusd":150}},
            {"id":format!("run:{key}"),"limit":{"tokens":1000,"cost_microusd":1000}}
        ],
        "reserved":{"tokens":100,"cost_microusd":100}
    }))
}
fn response() -> Result<ProviderResponse, serde_json::Error> {
    serde_json::from_value(json!({
        "id":"provider-call","model":"test-model",
        "message":{"role":"assistant","content":[{"type":"text","text":"{\"action\":\"r1c1\",\"reasoning\":\"A move.\"}"}]},
        "usage":{"input_tokens":20,"output_tokens":10,"cache_read_input_tokens":3,"cache_creation_5m_tokens":4,"cache_creation_1h_tokens":5},
        "stop_reason":"end_turn","latency_ms":12,"raw":{"unmodified":true}
    }))
}
async fn exercise(store: &(impl MatchStore + LlmLedger)) -> TestResult {
    store.create(record("ledger-a")).await?;
    store.create(record("ledger-b")).await?;
    let a = reservation("call-a", "ledger-a", "shared")?;
    let b = reservation("call-b", "ledger-b", "shared")?;
    let (left, right) = tokio::join!(store.reserve(a.clone()), store.reserve(b.clone()));
    assert_eq!(
        usize::from(left == Ok(ReserveResult::Reserved))
            + usize::from(right == Ok(ReserveResult::Reserved)),
        1
    );
    let (winner, loser) = if left == Ok(ReserveResult::Reserved) {
        assert_eq!(right, Err(LedgerError::BudgetExhausted));
        (a, b)
    } else {
        assert_eq!(left, Err(LedgerError::BudgetExhausted));
        (b, a)
    };
    assert!(store.load_call(&loser.id).await?.is_none());
    assert!(store
        .account(&format!("match:{}", loser.match_id))
        .await?
        .is_none());
    assert!(matches!(
        store.reserve(winner.clone()).await?,
        ReserveResult::Existing(_)
    ));
    let held = store
        .account("key:shared")
        .await?
        .ok_or("missing account")?;
    assert_eq!(held.held, winner.reserved);
    assert_eq!(held.spent, Charge::default());
    let mut changed = winner.clone();
    changed.request.system.push_str(" changed");
    assert_eq!(store.reserve(changed).await, Err(LedgerError::Conflict));
    let mut duplicate_slot = winner.clone();
    duplicate_slot.id = "different-id-same-turn-call".into();
    assert_eq!(
        store.reserve(duplicate_slot).await,
        Err(LedgerError::Conflict)
    );
    let unknown = store
        .mark_uncertain(&winner.id, ProviderError::Timeout)
        .await?;
    assert!(matches!(
        unknown.state,
        CallState::Uncertain {
            error: ProviderError::Timeout
        }
    ));
    assert_eq!(
        store
            .account("key:shared")
            .await?
            .ok_or("missing account")?
            .held,
        winner.reserved
    );
    assert!(
        matches!(store.reserve(winner.clone()).await?,ReserveResult::Existing(call) if matches!(call.state,CallState::Uncertain { .. }))
    );
    let result = response()?;
    let (first, second) = tokio::join!(
        store.settle(&winner.id, result.clone()),
        store.settle(&winner.id, result.clone())
    );
    assert_eq!(first?, second?);
    let settled = store.load_call(&winner.id).await?.ok_or("missing call")?;
    assert!(matches!(
        settled.state,
        CallState::Settled {
            charged: Charge {
                tokens: 42,
                cost_microusd: 42
            },
            exceeded_reservation: false,
            ..
        }
    ));
    let balance = store
        .account("key:shared")
        .await?
        .ok_or("missing account")?;
    assert_eq!(balance.held, Charge::default());
    assert_eq!(
        balance.spent,
        Charge {
            tokens: 42,
            cost_microusd: 42
        }
    );
    assert_eq!(
        store
            .mark_uncertain(&winner.id, ProviderError::Unavailable)
            .await?,
        settled
    );
    let mut changed = result;
    changed.id = "different-provider-response".into();
    assert_eq!(
        store.settle(&winner.id, changed).await,
        Err(LedgerError::Conflict)
    );
    assert_eq!(store.reserve(loser.clone()).await?, ReserveResult::Reserved);
    assert_eq!(
        store
            .account("key:shared")
            .await?
            .ok_or("missing account")?
            .held,
        Charge {
            tokens: 100,
            cost_microusd: 100
        }
    );
    // A later denied scope rolls back earlier account creation and never journals a call.
    store.create(record("ledger-denied")).await?;
    let mut denied = reservation("call-denied", "ledger-denied", "denied-key")?;
    denied.scopes[2].limit.tokens = 50;
    assert_eq!(
        store.reserve(denied).await,
        Err(LedgerError::BudgetExhausted)
    );
    assert!(store.load_call("call-denied").await?.is_none());
    assert!(store.account("key:denied-key").await?.is_none());
    assert!(store.account("match:ledger-denied").await?.is_none());
    let absent = reservation("absent", "no-such-match", "absent-key")?;
    assert_eq!(store.reserve(absent).await, Err(LedgerError::NotFound));
    assert!(store.account("key:absent-key").await?.is_none());
    // Transcript paging is seat-scoped and follows chronological call order.
    let first = store.calls(&winner.match_id, 0, None, 1).await?;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].reservation.id, winner.id);
    assert!(store
        .calls(&winner.match_id, 1, None, 100)
        .await?
        .is_empty());
    assert!(store
        .calls(
            &winner.match_id,
            0,
            Some(CallCursor {
                turn: 0,
                attempt: 0
            }),
            100
        )
        .await?
        .is_empty());
    assert_eq!(
        store.calls(&winner.match_id, 0, None, 101).await,
        Err(LedgerError::InvalidInput)
    );
    let mut overflow = reservation("overflow", "ledger-a", "overflow")?;
    overflow.reserved.tokens = u64::MAX;
    assert_eq!(
        store.reserve(overflow).await,
        Err(LedgerError::InvalidInput)
    );
    assert!(store.account("key:overflow").await?.is_none());
    Ok(())
}

async fn overrun_and_paging(store: &(impl MatchStore + LlmLedger)) -> TestResult {
    store.create(record("ledger-overrun")).await?;
    let original = reservation("overrun", "ledger-overrun", "overrun-key")?;
    store.reserve(original.clone()).await?;
    let mut unexpected = response()?;
    unexpected.usage.input_tokens = 200;
    let settled = store.settle("overrun", unexpected).await?;
    assert!(matches!(
        settled.state,
        CallState::Settled {
            exceeded_reservation: true,
            ..
        }
    ));
    let account = store
        .account("key:overrun-key")
        .await?
        .ok_or("missing account")?;
    assert_eq!(
        account.spent,
        Charge {
            tokens: 222,
            cost_microusd: 222
        }
    );
    assert_eq!(account.held, Charge::default());
    let mut next = original;
    next.id = "overrun-next".into();
    next.attempt = 1;
    assert_eq!(store.reserve(next).await, Err(LedgerError::BudgetExhausted));

    store.create(record("ledger-page")).await?;
    for (id, turn, attempt) in [("page-z", 0, 0), ("page-a", 0, 1), ("page-m", 2, 0)] {
        let mut call = reservation(id, "ledger-page", "paging")?;
        for scope in &mut call.scopes {
            scope.limit = Charge {
                tokens: 1000,
                cost_microusd: 1000,
            };
        }
        call.turn = turn;
        call.attempt = attempt;
        store.reserve(call).await?;
    }
    let first = store.calls("ledger-page", 0, None, 2).await?;
    assert_eq!(
        first
            .iter()
            .map(|call| call.reservation.id.as_str())
            .collect::<Vec<_>>(),
        vec!["page-z", "page-a"]
    );
    let next = store
        .calls(
            "ledger-page",
            0,
            Some(CallCursor {
                turn: 0,
                attempt: 1,
            }),
            2,
        )
        .await?;
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].reservation.id, "page-m");
    // A shared benchmark run caps independent provider keys atomically.
    store.create(record("run-ledger-a")).await?;
    store.create(record("run-ledger-b")).await?;
    let mut a = reservation("run-call-a", "run-ledger-a", "run-key-a")?;
    let mut b = reservation("run-call-b", "run-ledger-b", "run-key-b")?;
    for call in [&mut a, &mut b] {
        call.scopes[2] = BudgetScope {
            id: "run:shared-benchmark".into(),
            limit: Charge {
                tokens: 150,
                cost_microusd: 150,
            },
        };
    }
    let (left, right) = tokio::join!(store.reserve(a), store.reserve(b));
    assert_eq!(
        usize::from(left == Ok(ReserveResult::Reserved))
            + usize::from(right == Ok(ReserveResult::Reserved)),
        1
    );
    assert!(
        left == Err(LedgerError::BudgetExhausted) || right == Err(LedgerError::BudgetExhausted)
    );
    assert_eq!(
        store
            .account("run:shared-benchmark")
            .await?
            .ok_or("missing shared run")?
            .held,
        Charge {
            tokens: 100,
            cost_microusd: 100
        }
    );
    // A byte-limited page must not skip the next call, regardless of row IDs.
    let mut large = response()?;
    large.raw["large_fixture"] = json!("x".repeat(5 * 1024 * 1024));
    for id in ["page-z", "page-a", "page-m"] {
        store.settle(id, large.clone()).await?;
    }
    let first = store.calls("ledger-page", 0, None, 100).await?;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].reservation.id, "page-z");
    let second = store
        .calls(
            "ledger-page",
            0,
            Some(CallCursor {
                turn: 0,
                attempt: 0,
            }),
            100,
        )
        .await?;
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].reservation.id, "page-a");
    Ok(())
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_llm_reservations_are_atomic_idempotent_and_survive_restart() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("ledger.sqlite");
    let store = crate::SqliteMatchStore::open(&path).await?;
    exercise(&store).await?;
    overrun_and_paging(&store).await?;
    store.create(record("restart")).await?;
    let call = reservation("restart-call", "restart", "restart-key")?;
    store.reserve(call.clone()).await?;
    store
        .mark_uncertain(&call.id, ProviderError::Timeout)
        .await?;
    store.close().await;
    let reopened = crate::SqliteMatchStore::open(&path).await?;
    assert!(
        matches!(reopened.reserve(call.clone()).await?,ReserveResult::Existing(stored) if matches!(stored.state,CallState::Uncertain { .. }))
    );
    assert_eq!(
        reopened
            .account("key:restart-key")
            .await?
            .ok_or("missing persisted account")?
            .held,
        call.reserved
    );
    reopened.settle(&call.id, response()?).await?;
    assert_eq!(
        reopened
            .account("key:restart-key")
            .await?
            .ok_or("missing account")?
            .held,
        Charge::default()
    );
    reopened.close().await;
    Ok(())
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires GFA_TEST_POSTGRES_URL pointing at an isolated empty database"]
async fn postgres_llm_reservations_are_atomic_and_idempotent() -> TestResult {
    let url = std::env::var("GFA_TEST_POSTGRES_URL")?;
    let store = crate::PostgresMatchStore::connect(&url).await?;
    exercise(&store).await?;
    overrun_and_paging(&store).await?;
    store.create(record("restart")).await?;
    let call = reservation("restart-call", "restart", "restart-key")?;
    store.reserve(call.clone()).await?;
    store
        .mark_uncertain(&call.id, ProviderError::Timeout)
        .await?;
    store.close().await;
    let reopened = crate::PostgresMatchStore::connect(&url).await?;
    assert!(
        matches!(reopened.reserve(call.clone()).await?,ReserveResult::Existing(stored) if matches!(stored.state,CallState::Uncertain { .. }))
    );
    assert_eq!(
        reopened
            .account("key:restart-key")
            .await?
            .ok_or("missing persisted account")?
            .held,
        call.reserved
    );
    reopened.close().await;
    Ok(())
}

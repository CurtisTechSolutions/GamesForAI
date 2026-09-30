use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FrozenClock;
impl Clock for FrozenClock { fn now_ms(&self) -> u64 { 0 } }

fn fixture(mode: &str) -> Command {
    let mut command = Command::new("python3");
    command.args(["-u", "-c", include_str!("fixture.py"), mode]);
    command.env_clear();
    command
}
fn normal() -> Command { fixture("normal") }
fn hang() -> Command { fixture("hang") }
fn oversized() -> Command { fixture("oversized") }
fn invalid_utf8() -> Command { fixture("invalid_utf8") }
fn none() -> Command { fixture("none") }
fn pool(factory: fn() -> Command) -> Result<EnginePool, UciError> {
    let mut config = SandboxConfig::linux("/unused-test-engine");
    config.workers = 1;
    let mut pool = EnginePool::new(config)?;
    pool.fixture = Some(factory);
    Ok(pool)
}
fn position() -> UciPosition {
    UciPosition {
        initial_fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".into(),
        moves: vec![], chess960: false,
    }
}
fn limits(time_ms: u64) -> SearchLimits {
    SearchLimits { nodes: 100, depth: 3, time_ms, seed: 42 }
}

#[test]
fn reuses_process_and_preserves_pv_across_partial_info() -> Result<(), Box<dyn std::error::Error>> {
    let pool = pool(normal)?;
    let first = pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock)?;
    let second = pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock)?;
    assert_eq!(first.engine_name, second.engine_name);
    assert_eq!(second.best_move, "e2e4");
    assert_eq!(second.ponder.as_deref(), Some("e7e5"));
    assert_eq!(second.nodes, 43);
    assert_eq!(second.variations.len(), 1);
    assert_eq!(second.variations[0].pv, ["e2e4", "e7e5"]);
    Ok(())
}
#[test]
fn crashes_are_reaped_and_next_request_starts_a_new_worker() -> Result<(), Box<dyn std::error::Error>> {
    static LAUNCHES: AtomicUsize = AtomicUsize::new(0);
    fn flaky() -> Command {
        fixture(if LAUNCHES.fetch_add(1, Ordering::SeqCst) == 0 { "crash" } else { "normal" })
    }
    let pool = pool(flaky)?;
    assert_eq!(pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock), Err(UciError::Disconnected));
    let result = pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock)?;
    assert_eq!(result.best_move, "e2e4");
    assert_eq!(LAUNCHES.load(Ordering::SeqCst), 2);
    Ok(())
}
#[test]
fn hung_engine_is_killed_within_deadline_and_capacity_recovers() -> Result<(), Box<dyn std::error::Error>> {
    let mut pool = pool(hang)?;
    let started = Instant::now();
    assert_eq!(pool.search(&position(), &Settings::default(), limits(250), &FrozenClock), Err(UciError::Timeout));
    assert!(started.elapsed() < Duration::from_secs(2));
    pool.fixture = Some(normal);
    assert!(pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock).is_ok());
    Ok(())
}
#[test]
fn malformed_or_oversized_output_discards_the_worker() -> Result<(), Box<dyn std::error::Error>> {
    for factory in [oversized as fn() -> Command, invalid_utf8, none] {
        let mut pool = pool(factory)?;
        assert_eq!(pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock), Err(UciError::Protocol));
        pool.fixture = Some(normal);
        assert!(pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock).is_ok());
    }
    Ok(())
}
#[test]
fn occupied_pool_returns_busy_without_queueing() -> Result<(), Box<dyn std::error::Error>> {
    let pool = pool(normal)?;
    let _lease = pool.slots[0].lock().map_err(|_| "poisoned")?;
    let started = Instant::now();
    assert_eq!(pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock), Err(UciError::Busy));
    assert!(started.elapsed() < Duration::from_millis(100));
    Ok(())
}
#[test]
fn cancellation_releases_the_worker() -> Result<(), Box<dyn std::error::Error>> {
    struct Cancelled;
    impl Clock for Cancelled { fn now_ms(&self) -> u64 { u64::MAX } }
    let pool = pool(normal)?;
    assert_eq!(pool.search(&position(), &Settings::default(), limits(3000), &Cancelled), Err(UciError::Cancelled));
    assert!(pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock).is_ok());
    Ok(())
}
#[test]
fn blocked_stdin_cannot_hold_the_call_forever() -> Result<(), Box<dyn std::error::Error>> {
    let process = process::Process::spawn(fixture("blocked_input"))?;
    let deadline = Deadline::new(100, &FrozenClock)?;
    let text = "x".repeat(crate::MAX_LINE_BYTES);
    let mut outcome = Ok(());
    for _ in 0..64 {
        outcome = process.send(&text, &deadline);
        if outcome.is_err() { break; }
    }
    assert_eq!(outcome, Err(UciError::Timeout));
    drop(process);
    assert!(deadline.started.elapsed() < Duration::from_secs(2));
    Ok(())
}
#[test]
fn invalid_host_policy_and_missing_sandbox_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = SandboxConfig::linux("relative");
    assert!(matches!(EnginePool::new(config.clone()), Err(UciError::InvalidInput)));
    config.engine = "/does-not-exist-gfa-engine".into();
    let pool = EnginePool::new(config)?;
    assert_eq!(pool.search(&position(), &Settings::default(), limits(3000), &FrozenClock), Err(UciError::Unavailable));
    Ok(())
}

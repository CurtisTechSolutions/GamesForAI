//! Real installed-engine test; the dedicated Linux workflow supplies the sandbox dependencies.
use gfa_core::{Clock, SearchLimits, UciPosition};
use gfa_engine_uci::{EnginePool, SandboxConfig, Settings};
struct TestClock;
impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        0
    }
}

#[test]
#[ignore = "requires Linux user namespaces, bubblewrap, prlimit and Stockfish"]
fn sandboxed_stockfish_searches_and_reuses_process() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = SandboxConfig::linux("/usr/games/stockfish");
    config.workers = 1;
    let pool = EnginePool::new(config)?;
    let position = UciPosition {
        initial_fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".into(),
        moves: vec![],
        chess960: false,
    };
    let settings = Settings {
        multipv: 2,
        ..Settings::default()
    };
    let limits = SearchLimits {
        nodes: 1000,
        depth: 4,
        time_ms: 5000,
        seed: 7,
    };
    let first = pool.search(&position, &settings, limits, &TestClock)?;
    let second = pool.search(&position, &settings, limits, &TestClock)?;
    assert!(first.engine_name.contains("Stockfish"));
    assert_eq!(first.best_move, second.best_move);
    assert_eq!(first.variations.len(), 2);
    let legal = [
        "a2a3", "a2a4", "b2b3", "b2b4", "c2c3", "c2c4", "d2d3", "d2d4", "e2e3", "e2e4", "f2f3",
        "f2f4", "g2g3", "g2g4", "h2h3", "h2h4", "b1a3", "b1c3", "g1f3", "g1h3",
    ];
    assert!(legal.contains(&first.best_move.as_str()));
    Ok(())
}


#[test]
#[ignore = "requires the CI sandbox probe executable"]
fn sandbox_is_read_only_without_host_network_environment_or_capabilities() -> Result<(), Box<dyn std::error::Error>> {
    let engine = std::env::var("GFA_UCI_SANDBOX_PROBE")?;
    let pool = EnginePool::new(SandboxConfig::linux(engine))?;
    let position = UciPosition {
        initial_fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".into(),
        moves: vec![], chess960: false,
    };
    let result = pool.search(
        &position, &Settings::default(),
        SearchLimits { nodes: 100, depth: 2, time_ms: 5000, seed: 7 }, &TestClock,
    )?;
    let host_network = std::fs::read_link("/proc/self/ns/net")?;
    assert!(result.engine_name.starts_with("SandboxProbe net:["));
    assert!(!result.engine_name.contains(&*host_network.to_string_lossy()));
    assert_eq!(result.best_move, "e2e4");
    Ok(())
}

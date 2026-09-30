use super::*;
use gfa_core::UciPosition;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn capabilities() -> Result<Vec<EngineOption>, gfa_core::GameError> {
    [
        "option name Threads type spin default 1 min 1 max 1024",
        "option name Hash type spin default 16 min 1 max 33554432",
        "option name MultiPV type spin default 1 min 1 max 256",
        "option name Skill Level type spin default 20 min 0 max 20",
        "option name Ponder type check default false",
        "option name UCI_Chess960 type check default false",
        "option name UCI_LimitStrength type check default false",
        "option name UCI_Elo type spin default 1320 min 1320 max 3190",
        "option name UCI_ShowWDL type check default false",
        "option name Debug Log File type string default <empty>",
    ]
    .iter()
    .map(|line| match parse_line(line)? {
        EngineLine::Option(option) => Ok(option),
        _ => Err(super::invalid("Expected option")),
    })
    .collect()
}

#[test]
fn parses_negotiation_progress_mate_bounds_and_bestmove() -> TestResult {
    assert_eq!(parse_line("uciok\r\n")?, EngineLine::UciOk);
    assert_eq!(parse_line("readyok")?, EngineLine::ReadyOk);
    assert_eq!(
        parse_line("id name Stockfish 19")?,
        EngineLine::Name("Stockfish 19".into())
    );
    assert!(matches!(
        parse_line("info string NNUE loaded")?,
        EngineLine::Ignored
    ));
    let EngineLine::Info(info) = parse_line("info depth 16 seldepth 22 multipv 2 score cp -43 upperbound nodes 27182 nps 200000 time 135 wdl 100 500 400 pv e2e4 e7e5 g1f3")? else { return Err("info".into()); };
    assert_eq!(info.depth, Some(16));
    assert_eq!(info.multipv, Some(2));
    assert_eq!(info.score, Some(Score::Centipawns(-43)));
    assert_eq!(info.bound, Bound::Upper);
    assert_eq!(info.nodes, Some(27182));
    assert_eq!(info.time_ms, Some(135));
    assert_eq!(info.wdl, Some([100, 500, 400]));
    assert_eq!(info.pv, ["e2e4", "e7e5", "g1f3"]);
    let EngineLine::Info(info) = parse_line("info depth 8 score mate -3 lowerbound pv e1d1")?
    else {
        return Err("mate".into());
    };
    assert_eq!(info.score, Some(Score::Mate(-3)));
    assert_eq!(info.bound, Bound::Lower);
    assert_eq!(
        parse_line("bestmove a7a8n ponder e8d7")?,
        EngineLine::BestMove {
            action: Some("a7a8n".into()),
            ponder: Some("e8d7".into())
        }
    );
    assert_eq!(
        parse_line("bestmove (none)")?,
        EngineLine::BestMove {
            action: None,
            ponder: None
        }
    );
    Ok(())
}

#[test]
fn commands_preserve_history_and_reject_injected_or_oversized_tokens() -> TestResult {
    let position = UciPosition {
        initial_fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".into(),
        moves: vec!["e2e4".into(), "e7e5".into()],
        chess960: false,
    };
    assert!(position_command(&position)?.ends_with(" moves e2e4 e7e5"));
    for bad in [
        "e2e4\nquit",
        "0000",
        "e2e2",
        "a7a8k",
        "a7a8N",
        "e2e4 go infinite",
    ] {
        let mut bad_position = position.clone();
        bad_position.moves = vec![bad.into()];
        assert!(position_command(&bad_position).is_err(), "{bad}");
    }
    let mut bad = position.clone();
    bad.initial_fen.push_str("\nquit");
    assert!(position_command(&bad).is_err());
    bad = position;
    bad.moves = vec!["e2e4".into(); 1001];
    assert!(position_command(&bad).is_err());
    let limits = SearchLimits {
        nodes: 100,
        depth: 4,
        time_ms: 50,
        seed: 0,
    };
    assert_eq!(go_command(limits)?, "go nodes 100 depth 4 movetime 50");
    assert!(go_command(SearchLimits {
        time_ms: 0,
        ..limits
    })
    .is_err());
    Ok(())
}

#[test]
fn pooled_options_reset_strength_and_respect_advertised_ranges() -> TestResult {
    let available = capabilities()?;
    let settings = Settings {
        skill: Some(5),
        multipv: 3,
        ..Settings::default()
    };
    let commands = settings.commands(&available, false)?;
    assert!(commands.contains(&"setoption name Skill Level value 5".into()));
    assert!(commands.contains(&"setoption name UCI_LimitStrength value false".into()));
    assert!(commands.contains(&"setoption name MultiPV value 3".into()));
    assert!(commands.iter().all(|s| !s.contains("File")));
    assert!(Settings::default()
        .commands(&available, false)?
        .contains(&"setoption name Skill Level value 20".into()));
    let elo = Settings {
        elo: Some(1800),
        ..Settings::default()
    };
    assert!(elo
        .commands(&available, true)?
        .contains(&"setoption name UCI_LimitStrength value true".into()));
    for bad in [
        Settings {
            elo: Some(1200),
            ..Settings::default()
        },
        Settings {
            elo: Some(1800),
            skill: Some(1),
            ..Settings::default()
        },
        Settings {
            threads: 0,
            ..Settings::default()
        },
        Settings {
            hash_mb: 257,
            ..Settings::default()
        },
        Settings {
            multipv: 17,
            ..Settings::default()
        },
        Settings {
            skill: Some(21),
            ..Settings::default()
        },
    ] {
        assert!(bad.commands(&available, false).is_err());
    }
    assert!(Settings::default().commands(&[], false).is_err());
    Ok(())
}

#[test]
fn malformed_output_is_bounded_and_never_panics() {
    for line in [
        "bestmove e2e9",
        "bestmove",
        "bestmove e2e4 extra",
        "info depth",
        "info nodes -1",
        "info depth 9999999",
        "info multipv 0",
        "info score cp NaN",
        "info score surprise 1",
        "info wdl 1000 1000 1000",
        "info pv e2e4 quit",
        "option name Threads type spin default 4 min 1 max 2",
        "option name Ponder type check default maybe",
        "readyok\0",
        "uciok\nquit",
    ] {
        assert!(parse_line(line).is_err(), "{line}");
    }
    assert!(parse_line(&"x".repeat(MAX_LINE_BYTES + 1)).is_err());
    assert!(parse_line(&format!("info pv {}", vec!["e2e4"; 129].join(" "))).is_err());
    for length in 0..32 {
        let text = "e".repeat(length);
        assert!(!super::valid_move(&text));
    }
}

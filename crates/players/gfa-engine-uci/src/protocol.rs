use crate::{invalid, valid_move, MAX_LINE_BYTES};
use gfa_core::GameError;

/// Meaning of the engine's score, always from the root side to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Score {
    /// Centipawn evaluation; not a game return or a measured win probability.
    Centipawns(i32),
    /// Signed number of moves to mate; positive favors the root player.
    Mate(i32),
}

/// Whether a reported score is exact within the completed search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bound {
    /// No bound modifier.
    #[default]
    Exact,
    /// The score is at least this value.
    Lower,
    /// The score is at most this value.
    Upper,
}

/// Search progress for one variation; missing fields remain absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    /// Completed nominal depth.
    pub depth: Option<u16>,
    /// Cumulative node count.
    pub nodes: Option<u64>,
    /// Search milliseconds reported by the engine.
    pub time_ms: Option<u64>,
    /// One-based MultiPV rank; absent means the principal variation.
    pub multipv: Option<u8>,
    /// Typed root-side evaluation.
    pub score: Option<Score>,
    /// Score bound.
    pub bound: Bound,
    /// Win/draw/loss permille, when supported; total must equal 1000.
    pub wdl: Option<[u16; 3]>,
    /// Validated move tokens, at most 128. Legality is checked by the game.
    pub pv: Vec<String>,
}

/// Recognized UCI option types. File/string options are never sent to an engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionKind {
    /// Bounded integer option.
    Spin {
        /// Default value.
        default: i64,
        /// Smallest accepted value.
        min: i64,
        /// Largest accepted value.
        max: i64,
    },
    /// Boolean option.
    Check {
        /// Default value.
        default: bool,
    },
    /// An action with no value.
    Button,
    /// A string/combo option retained only as a capability name.
    Other,
}

/// Advertised capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineOption {
    /// Engine option name.
    pub name: String,
    /// Type and bounds.
    pub kind: OptionKind,
}

/// A bounded, parsed engine response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineLine {
    /// Completed UCI negotiation.
    UciOk,
    /// Completed synchronization barrier.
    ReadyOk,
    /// Engine identity.
    Name(String),
    /// Engine author.
    Author(String),
    /// Advertised option.
    Option(EngineOption),
    /// Search progress.
    Info(Info),
    /// Final move; None for 0000/(none), which is valid only when no legal move exists.
    BestMove {
        /// Selected UCI move.
        action: Option<String>,
        /// Optional suggested reply.
        ponder: Option<String>,
    },
    /// Unrecognized or informational text, intentionally discarded.
    Ignored,
}

/// Parse one output line without retaining unbounded logs or accepting control sequences.
pub fn parse_line(line: &str) -> Result<EngineLine, GameError> {
    if line.len() > MAX_LINE_BYTES { return Err(invalid("UCI line exceeds 8192 bytes")); }
    let line = line.trim_end_matches(['\r','\n']);
    if line.chars().any(|c| c.is_control() && c != '\t') {
        return Err(invalid("UCI line contains a control character"));
    }
    let words: Vec<_> = line.split_ascii_whitespace().collect();
    match words.as_slice() {
        ["uciok"] => Ok(EngineLine::UciOk),
        ["readyok"] => Ok(EngineLine::ReadyOk),
        ["id", "name", rest @ ..] => Ok(EngineLine::Name(rest.join(" "))),
        ["id", "author", rest @ ..] => Ok(EngineLine::Author(rest.join(" "))),
        ["option", "name", ..] => option(&words).map(EngineLine::Option),
        ["info", "string", ..] => Ok(EngineLine::Ignored),
        ["info", rest @ ..] => info(rest).map(EngineLine::Info),
        ["bestmove", action] => Ok(EngineLine::BestMove { action: move_token(action)?, ponder: None }),
        ["bestmove", action, "ponder", reply] => Ok(EngineLine::BestMove { action: move_token(action)?, ponder: move_token(reply)? }),
        ["bestmove", ..] => Err(invalid("Malformed UCI bestmove")),
        _ => Ok(EngineLine::Ignored),
    }
}

fn move_token(text: &str) -> Result<Option<String>, GameError> {
    if matches!(text, "0000" | "(none)") { Ok(None) }
    else if valid_move(text) { Ok(Some(text.into())) }
    else { Err(invalid("Invalid UCI move token")) }
}

fn option(words: &[&str]) -> Result<EngineOption, GameError> {
    let split = words.iter().position(|&word| word == "type").ok_or_else(|| invalid("UCI option has no type"))?;
    if split <= 2 || split + 1 >= words.len() { return Err(invalid("Malformed UCI option")); }
    let name = words[2..split].join(" ");
    if name.len() > 128 { return Err(invalid("UCI option name too long")); }
    let value = |key: &str| {
        words[split+2..].windows(2).find(|pair| pair[0] == key).map(|pair| pair[1])
            .ok_or_else(|| invalid("Missing UCI option bound/default"))
    };
    let number = |key: &str| -> Result<i64, GameError> {
        value(key)?.parse().map_err(|_| invalid("Invalid UCI option number"))
    };
    let kind = match words[split+1] {
        "spin" => {
            let (default, min, max) = (number("default")?, number("min")?, number("max")?);
            if min > max || !(min..=max).contains(&default) { return Err(invalid("Inconsistent UCI option range")); }
            OptionKind::Spin { default, min, max }
        }
        "check" => OptionKind::Check { default: match value("default")? {
            "true" => true, "false" => false, _ => return Err(invalid("Invalid UCI boolean")),
        }},
        "button" => OptionKind::Button,
        "string" | "combo" => OptionKind::Other,
        _ => return Err(invalid("Unknown UCI option type")),
    };
    Ok(EngineOption { name, kind })
}

fn info(words: &[&str]) -> Result<Info, GameError> {
    let mut result = Info::default();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        i += 1;
        if word == "pv" {
            if words.len()-i > 128 || words[i..].iter().any(|m| !valid_move(m)) {
                return Err(invalid("Invalid or oversized UCI principal variation"));
            }
            result.pv = words[i..].iter().map(|s| (*s).into()).collect();
            break;
        }
        if word == "string" { break; }
        if word == "lowerbound" { result.bound = Bound::Lower; continue; }
        if word == "upperbound" { result.bound = Bound::Upper; continue; }
        let value = |index: usize| words.get(index).copied().ok_or_else(|| invalid("Missing UCI info value"));
        let numeric = |index: usize| -> Result<u64, GameError> {
            value(index)?.parse().map_err(|_| invalid("Invalid UCI info number"))
        };
        match word {
            "depth" => { result.depth = Some(u16::try_from(numeric(i)?).map_err(|_| invalid("Depth overflow"))?); i += 1; }
            "nodes" => { result.nodes = Some(numeric(i)?); i += 1; }
            "time" => { result.time_ms = Some(numeric(i)?); i += 1; }
            "multipv" => {
                let rank = u8::try_from(numeric(i)?).map_err(|_| invalid("MultiPV overflow"))?;
                if rank == 0 { return Err(invalid("MultiPV must be positive")); }
                result.multipv = Some(rank); i += 1;
            }
            "score" => {
                let kind = value(i)?;
                let score: i32 = value(i+1)?.parse().map_err(|_| invalid("Invalid UCI score"))?;
                result.score = Some(match kind { "cp" => Score::Centipawns(score), "mate" => Score::Mate(score), _ => return Err(invalid("Unknown UCI score type")) });
                i += 2;
            }
            "wdl" => {
                let mut wdl = [0_u16;3];
                for (offset, target) in wdl.iter_mut().enumerate() {
                    *target = u16::try_from(numeric(i+offset)?).map_err(|_| invalid("WDL overflow"))?;
                }
                if wdl.iter().map(|&n| u32::from(n)).sum::<u32>() != 1000 { return Err(invalid("WDL must total 1000")); }
                result.wdl = Some(wdl); i += 3;
            }
            _ => {},
        }
    }
    Ok(result)
}

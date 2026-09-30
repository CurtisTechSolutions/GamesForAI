//! Bounded UCI command and response types. Process isolation is supplied by the host.
mod protocol;
mod pool;
pub use pool::{EnginePool, SandboxConfig, SearchResult, UciError};
use gfa_core::serde_json::Value;
use gfa_core::{DynGame, ErrorCode, GameError, PlayerTurn, SearchLimits, UciPosition, Viewer};
pub use protocol::{parse_line, Bound, EngineLine, EngineOption, Info, OptionKind, Score};

/// Maximum bytes accepted in an engine output line or command.
pub const MAX_LINE_BYTES: usize = 8192;

/// Reconstruct UCI input solely from the acting player's visible information.
pub fn planning_position(
    game: &dyn DynGame,
    config: &Value,
    turn: &PlayerTurn<'_>,
    seed: u64,
) -> Result<UciPosition, GameError> {
    if !game.supports_uci() {
        return Err(invalid("This game does not support UCI"));
    }
    let state =
        game.state_from_observation(config, turn.observation, Viewer::Player(turn.seat), seed)?;
    if game.is_terminal(&state)?
        || game.current_players(&state)? != [turn.seat]
        || game.legal_actions(&state, turn.seat)? != turn.legal_actions
    {
        return Err(GameError::illegal(
            "Observation and legal actions do not describe this turn",
        ));
    }
    let position = game
        .uci_position(&state)?
        .ok_or_else(|| invalid("Engine omitted UCI position"))?;
    position_command(&position)?;
    Ok(position)
}

/// Serialize an engine-validated position, rejecting control characters and oversized input.
/// Legality must already have been checked by the rules engine, as in planning_position.
pub fn position_command(position: &UciPosition) -> Result<String, GameError> {
    let text = &position.initial_fen;
    let fields: Vec<_> = text.split(' ').collect();
    if text.len() > 128
        || fields.len() != 6
        || !fields[0]
            .bytes()
            .all(|b| b"prnbqkPRNBQK12345678/".contains(&b))
        || fields[0].split('/').count() != 8
        || !matches!(fields[1], "w" | "b")
        || !fields[2]
            .bytes()
            .all(|b| b"KQkqABCDEFGHabcdefgh-".contains(&b))
        || !(fields[3] == "-" || square(fields[3].as_bytes()))
        || fields[4].parse::<u32>().is_err()
        || fields[5].parse::<u32>().ok().is_none_or(|n| n == 0)
        || position.moves.len() > 1000
        || position.moves.iter().any(|m| !valid_move(m))
    {
        return Err(invalid("Invalid UCI position input"));
    }
    let mut command = format!("position fen {text}");
    if !position.moves.is_empty() {
        command.push_str(" moves ");
        command.push_str(&position.moves.join(" "));
    }
    if command.len() > MAX_LINE_BYTES {
        return Err(invalid("UCI position exceeds 8192 bytes"));
    }
    Ok(command)
}

/// Resource-bounded UCI search. The process supervisor must also enforce a hard deadline.
pub fn go_command(limits: SearchLimits) -> Result<String, GameError> {
    let limits = limits.validate()?;
    Ok(format!(
        "go nodes {} depth {} movetime {}",
        limits.nodes, limits.depth, limits.time_ms
    ))
}

/// UCI settings that can be changed without accepting arbitrary commands or file paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Threads per engine, 1..=4.
    pub threads: u8,
    /// Hash size in MiB, 1..=256.
    pub hash_mb: u16,
    /// Analysis variations, 1..=16.
    pub multipv: u8,
    /// Optional strength setting, 0..=20.
    pub skill: Option<u8>,
    /// Optional target engine Elo; checked against the engine's advertised range.
    pub elo: Option<u16>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            threads: 1,
            hash_mb: 32,
            multipv: 1,
            skill: None,
            elo: None,
        }
    }
}

impl Settings {
    /// Generate only allowlisted options, validating values against advertised capabilities.
    pub fn commands(
        &self,
        available: &[EngineOption],
        chess960: bool,
    ) -> Result<Vec<String>, GameError> {
        if !(1..=4).contains(&self.threads)
            || !(1..=256).contains(&self.hash_mb)
            || !(1..=16).contains(&self.multipv)
            || self.skill.is_some_and(|n| n > 20)
            || (self.skill.is_some() && self.elo.is_some())
        {
            return Err(invalid("Invalid or conflicting UCI settings"));
        }
        let spin = |name: &str, value: i64| -> Result<String, GameError> {
            match available.iter().find(|o| o.name == name).map(|o| &o.kind) {
                Some(OptionKind::Spin { min, max, .. }) if (*min..=*max).contains(&value) => {
                    Ok(format!("setoption name {name} value {value}"))
                }
                _ => Err(invalid(&format!(
                    "Engine does not support {name} at the requested value"
                ))),
            }
        };
        let check = |name: &str, value: bool| -> Result<Option<String>, GameError> {
            match available.iter().find(|o| o.name == name).map(|o| &o.kind) {
                Some(OptionKind::Check { .. }) => {
                    Ok(Some(format!("setoption name {name} value {value}")))
                }
                None if !value => Ok(None),
                _ => Err(invalid(&format!("Engine does not support {name}"))),
            }
        };
        let mut commands = vec![
            spin("Threads", i64::from(self.threads))?,
            spin("Hash", i64::from(self.hash_mb))?,
        ];
        if available.iter().any(|o| o.name == "MultiPV") || self.multipv != 1 {
            commands.push(spin("MultiPV", i64::from(self.multipv))?);
        }
        for (name, value) in [
            ("Ponder", false),
            ("UCI_Chess960", chess960),
            ("UCI_LimitStrength", self.elo.is_some()),
        ] {
            if let Some(command) = check(name, value)? {
                commands.push(command);
            }
        }
        if let Some(elo) = self.elo {
            commands.push(spin("UCI_Elo", i64::from(elo))?);
        }
        // Reset a pooled engine's former skill, even when this request asks for full strength.
        if available.iter().any(|o| o.name == "Skill Level") || self.skill.is_some() {
            commands.push(spin("Skill Level", i64::from(self.skill.unwrap_or(20)))?);
        }
        if available.iter().any(|o| o.name == "UCI_ShowWDL") {
            if let Some(command) = check("UCI_ShowWDL", true)? {
                commands.push(command);
            }
        }
        Ok(commands)
    }
}

pub(crate) fn valid_move(text: &str) -> bool {
    let b = text.as_bytes();
    (b.len() == 4 || b.len() == 5)
        && square(&b[..2])
        && square(&b[2..4])
        && b[..2] != b[2..4]
        && (b.len() == 4 || b"qrbn".contains(&b[4]))
}

fn square(b: &[u8]) -> bool {
    b.len() == 2 && (b'a'..=b'h').contains(&b[0]) && (b'1'..=b'8').contains(&b[1])
}

pub(crate) fn invalid(message: &str) -> GameError {
    GameError::new(
        ErrorCode::InvalidConfig,
        message,
        "Use a compatible installed UCI engine and validated, bounded settings.",
    )
}

#[cfg(test)]
mod tests;

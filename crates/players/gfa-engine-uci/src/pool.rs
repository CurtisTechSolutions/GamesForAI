//! Synchronous bounded workers, intended to run on the host's blocking executor.
use crate::{go_command, parse_line, position_command, EngineLine, EngineOption, Info, Settings};
use gfa_core::{Clock, SearchLimits, UciPosition};
use std::{
    collections::BTreeMap,
    fmt,
    process::Command,
    sync::{Mutex, TryLockError},
    time::{Duration, Instant},
};

mod process;
mod sandbox;
pub use sandbox::SandboxConfig;

/// Operational failures stay separate from errors in the player's game action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UciError {
    /// Every configured worker is leased; no unbounded waiting queue is created.
    Busy,
    /// Invalid input or unsupported engine option.
    InvalidInput,
    /// Linux isolation tools or the engine executable are unavailable.
    Unavailable,
    /// Engine exited or a pipe failed.
    Disconnected,
    /// Malformed, excessive or out-of-sequence engine output.
    Protocol,
    /// The hard request deadline expired.
    Timeout,
    /// The host clock signalled cancellation with u64::MAX.
    Cancelled,
}
impl fmt::Display for UciError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Busy => "All UCI workers are busy",
            Self::InvalidInput => "Invalid or unsupported UCI input",
            Self::Unavailable => "Sandboxed UCI engine is unavailable",
            Self::Disconnected => "UCI engine disconnected",
            Self::Protocol => "UCI engine returned invalid or excessive output",
            Self::Timeout => "UCI request exceeded its wall-clock budget",
            Self::Cancelled => "UCI request was cancelled",
        })
    }
}
impl std::error::Error for UciError {}

/// Engine output before game-specific legality checks. Never expose it directly as an action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    /// Engine identity, bounded by the protocol line limit.
    pub engine_name: String,
    /// Selected UCI token; callers must check it against the legal list.
    pub best_move: String,
    /// Optional reply; callers must validate it in the resulting position.
    pub ponder: Option<String>,
    /// Latest complete PV for each rank, ordered by rank.
    pub variations: Vec<Info>,
    /// Last reported cumulative search nodes.
    pub nodes: u64,
    /// Total host wall time, including startup and synchronization.
    pub elapsed_ms: u64,
}

struct Deadline<'a> {
    end: Instant,
    started: Instant,
    clock: &'a dyn Clock,
    clock_start: u64,
    budget_ms: u64,
}
impl<'a> Deadline<'a> {
    fn new(budget_ms: u64, clock: &'a dyn Clock) -> Result<Self, UciError> {
        let clock_start = clock.now_ms();
        if clock_start == u64::MAX {
            return Err(UciError::Cancelled);
        }
        let started = Instant::now();
        Ok(Self {
            end: started + Duration::from_millis(budget_ms),
            started,
            clock,
            clock_start,
            budget_ms,
        })
    }
    fn check(&self) -> Result<Duration, UciError> {
        let now = self.clock.now_ms();
        if now == u64::MAX {
            return Err(UciError::Cancelled);
        }
        if now.saturating_sub(self.clock_start) >= self.budget_ms {
            return Err(UciError::Timeout);
        }
        self.end.checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(UciError::Timeout)
    }
    fn tick(&self) -> Result<Duration, UciError> {
        Ok(self.check()?.min(Duration::from_millis(10)))
    }
    fn startup(&self, milliseconds: u64) -> Self {
        Self {
            end: self.end.min(Instant::now() + Duration::from_millis(milliseconds)),
            started: self.started,
            clock: self.clock,
            clock_start: self.clock_start,
            budget_ms: self.budget_ms,
        }
    }
}

/// Reused engine slots. Failed, cancelled and timed-out processes are killed before release.
/// Constructing a pool performs no I/O; the first request starts a sandboxed engine.
pub struct EnginePool {
    config: SandboxConfig,
    slots: Vec<Mutex<Option<Worker>>>,
    #[cfg(test)]
    fixture: Option<fn() -> Command>,
}
impl EnginePool {
    /// Validate host resource policy. There is no production unsandboxed launch mode.
    pub fn new(config: SandboxConfig) -> Result<Self, UciError> {
        config.validate()?;
        Ok(Self {
            slots: (0..config.workers).map(|_| Mutex::new(None)).collect(),
            config,
            #[cfg(test)]
            fixture: None,
        })
    }

    fn command(&self) -> Result<Command, UciError> {
        #[cfg(test)]
        if let Some(fixture) = self.fixture {
            return Ok(fixture());
        }
        self.config.command()
    }

    /// Search an already game-validated position. Settings and input are independently bounded.
    /// The caller must validate bestmove/PV with its rules engine before committing or publishing.
    /// The deadline includes process startup, option synchronization and the search itself.
    pub fn search(
        &self,
        position: &UciPosition,
        settings: &Settings,
        limits: SearchLimits,
        clock: &dyn Clock,
    ) -> Result<SearchResult, UciError> {
        limits.validate().map_err(|_| UciError::InvalidInput)?;
        let position_text = position_command(position).map_err(|_| UciError::InvalidInput)?;
        let deadline = Deadline::new(limits.time_ms, clock)?;
        for slot in &self.slots {
            let mut lease = match slot.try_lock() {
                Ok(lease) => lease,
                Err(TryLockError::WouldBlock) => continue,
                Err(TryLockError::Poisoned(_)) => return Err(UciError::Unavailable),
            };
            let outcome = (|| {
                if lease.is_none() {
                    let mut worker = Worker::spawn(self.command()?)?;
                    worker.handshake(&deadline.startup(self.config.startup_ms))?;
                    *lease = Some(worker);
                }
                lease.as_mut().ok_or(UciError::Unavailable)?
                    .search(&position_text, position.chess960, settings, limits, &deadline)
            })();
            if outcome.is_err() {
                // Dropping a worker kills and reaps its process before another request gets the slot.
                *lease = None;
            }
            return outcome;
        }
        Err(UciError::Busy)
    }
}

struct Worker {
    io: process::Process,
    name: String,
    options: Vec<EngineOption>,
}
impl Worker {
    fn spawn(command: Command) -> Result<Self, UciError> {
        Ok(Self { io: process::Process::spawn(command)?, name: String::new(), options: vec![] })
    }
    fn handshake(&mut self, deadline: &Deadline<'_>) -> Result<(), UciError> {
        self.io.send("uci", deadline)?;
        let mut budget = OutputBudget::default();
        loop {
            match self.line(deadline, &mut budget)? {
                EngineLine::Name(name) => self.name = name,
                EngineLine::Option(option) if self.options.len() < 128 => {
                    if self.options.iter().any(|o| o.name == option.name) {
                        return Err(UciError::Protocol);
                    }
                    self.options.push(option);
                }
                EngineLine::UciOk => break,
                EngineLine::Author(_) | EngineLine::Ignored => {}
                _ => return Err(UciError::Protocol),
            }
        }
        if self.name.is_empty() {
            return Err(UciError::Protocol);
        }
        self.ready(deadline, &mut budget)
    }
    fn line(&mut self, deadline: &Deadline<'_>, budget: &mut OutputBudget) -> Result<EngineLine, UciError> {
        let line = self.io.receive(deadline)?;
        budget.lines += 1;
        budget.bytes += line.len();
        if budget.lines > 20_000 || budget.bytes > 4 * 1024 * 1024 {
            return Err(UciError::Protocol);
        }
        parse_line(&line).map_err(|_| UciError::Protocol)
    }
    fn ready(&mut self, deadline: &Deadline<'_>, budget: &mut OutputBudget) -> Result<(), UciError> {
        self.io.send("isready", deadline)?;
        loop {
            match self.line(deadline, budget)? {
                EngineLine::ReadyOk => return Ok(()),
                EngineLine::Ignored => {}
                _ => return Err(UciError::Protocol),
            }
        }
    }
    fn search(
        &mut self,
        position: &str,
        chess960: bool,
        settings: &Settings,
        mut limits: SearchLimits,
        deadline: &Deadline<'_>,
    ) -> Result<SearchResult, UciError> {
        let mut budget = OutputBudget::default();
        for command in settings.commands(&self.options, chess960).map_err(|_| UciError::InvalidInput)? {
            self.io.send(&command, deadline)?;
        }
        // Reset transposition state between independent decisions, while reusing the OS process.
        self.io.send("ucinewgame", deadline)?;
        self.ready(deadline, &mut budget)?;
        self.io.send(position, deadline)?;
        limits.time_ms = u64::try_from(deadline.check()?.as_millis()).unwrap_or(60_000)
            .saturating_sub(10).max(1).min(limits.time_ms);
        self.io.send(&go_command(limits).map_err(|_| UciError::InvalidInput)?, deadline)?;
        let mut variations = BTreeMap::new();
        let mut nodes = 0;
        loop {
            match self.line(deadline, &mut budget)? {
                EngineLine::Info(info) => {
                    if let Some(count) = info.nodes { nodes = nodes.max(count); }
                    let rank = info.multipv.unwrap_or(1);
                    if rank > settings.multipv { return Err(UciError::Protocol); }
                    if !info.pv.is_empty() { variations.insert(rank, info); }
                }
                EngineLine::BestMove { action: Some(best_move), ponder } => {
                    // Reject an already expired response even if it was buffered before the deadline.
                    deadline.check()?;
                    return Ok(SearchResult {
                        engine_name: self.name.clone(),
                        best_move,
                        ponder,
                        variations: variations.into_values().collect(),
                        nodes,
                        elapsed_ms: u64::try_from(deadline.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    });
                }
                EngineLine::Ignored => {}
                _ => return Err(UciError::Protocol),
            }
        }
    }
}
#[derive(Default)]
struct OutputBudget { lines: usize, bytes: usize }

#[cfg(all(test, unix))]
mod tests;

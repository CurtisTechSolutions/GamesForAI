use super::UciError;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Trusted host policy; none of these paths or limits are accepted from a game action.
#[derive(Clone, Debug)]
pub struct SandboxConfig {
    /// Absolute path to an installed engine with embedded evaluation data.
    pub engine: PathBuf,
    /// Absolute path to bubblewrap.
    pub bubblewrap: PathBuf,
    /// Absolute path to util-linux prlimit.
    pub prlimit: PathBuf,
    /// Concurrent engine processes, 1..=4.
    pub workers: u8,
    /// Maximum address space per process in MiB, 128..=4096.
    pub memory_mb: u32,
    /// Hard cumulative CPU seconds per process lifetime, 10..=600.
    /// Expiry recycles the worker on its next request.
    pub cpu_seconds: u32,
    /// Maximum negotiation time, within the request deadline, 1..=5000 ms.
    pub startup_ms: u64,
}
impl SandboxConfig {
    /// Linux defaults. Engine availability and namespace support are checked on first launch.
    pub fn linux(engine: impl Into<PathBuf>) -> Self {
        Self {
            engine: engine.into(),
            bubblewrap: "/usr/bin/bwrap".into(),
            prlimit: "/usr/bin/prlimit".into(),
            workers: 2,
            memory_mb: 2048,
            cpu_seconds: 120,
            startup_ms: 3000,
        }
    }
    pub(super) fn validate(&self) -> Result<(), UciError> {
        if !self.engine.is_absolute()
            || !self.bubblewrap.is_absolute()
            || !self.prlimit.is_absolute()
            || !(1..=4).contains(&self.workers)
            || !(128..=4096).contains(&self.memory_mb)
            || !(10..=600).contains(&self.cpu_seconds)
            || !(1..=5000).contains(&self.startup_ms)
        {
            return Err(UciError::InvalidInput);
        }
        Ok(())
    }
    pub(super) fn command(&self) -> Result<Command, UciError> {
        self.validate()?;
        if !cfg!(target_os = "linux") {
            return Err(UciError::Unavailable);
        }
        let engine = canonical_file(&self.engine)?;
        let bubblewrap = canonical_file(&self.bubblewrap)?;
        let prlimit = canonical_file(&self.prlimit)?;
        let mut command = Command::new(prlimit);
        command.env_clear().current_dir("/");
        command.args([
            format!("--as={}", u64::from(self.memory_mb) * 1024 * 1024),
            format!("--cpu={}", self.cpu_seconds),
            "--nofile=64".into(),
            "--core=0".into(),
            "--fsize=0".into(),
            "--".into(),
        ]);
        command.arg(bubblewrap).args([
            "--unshare-all",
            "--unshare-user",
            "--disable-userns",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
            "--clearenv",
        ]);
        // Mount only system runtime files. Never mount home, the match store, host sockets or /etc.
        // Stockfish packages embed NNUE data; engines needing other files require an explicit future policy.
        for path in ["/usr", "/bin", "/lib", "/lib64"] {
            if Path::new(path).exists() {
                command.args(["--ro-bind", path, path]);
            }
        }
        command.arg("--ro-bind").arg(engine).arg("/engine");
        command.args([
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--chdir",
            "/",
            "--remount-ro",
            "/proc",
            "--remount-ro",
            "/dev",
            "--remount-ro",
            "/",
            "--",
            "/engine",
        ]);
        Ok(command)
    }
}
fn canonical_file(path: &Path) -> Result<PathBuf, UciError> {
    let path = path.canonicalize().map_err(|_| UciError::Unavailable)?;
    if !path.is_file() {
        return Err(UciError::Unavailable);
    }
    Ok(path)
}

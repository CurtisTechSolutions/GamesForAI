//! Reuse the installed-engine sandbox for local Python training.
use crate::{error, NativeEnv};
use gfa_core::{Opponent, PlayerTurn, SearchLimits, Viewer};
use gfa_engine_uci::{EnginePool, SandboxConfig, Settings, UciOpponent};
use pyo3::prelude::*;
use std::sync::Arc;

/// Explicit host-owned pool. An environment observation never controls its executable.
#[pyclass(module = "gamesforai._native")]
pub(crate) struct NativeUciPool {
    inner: Arc<EnginePool>,
}

#[pymethods]
impl NativeUciPool {
    #[new]
    #[pyo3(signature = (engine_path, workers=1))]
    fn new(engine_path: &str, workers: u8) -> PyResult<Self> {
        if !cfg!(target_os = "linux") {
            return Err(error("Stockfish training requires the Linux engine sandbox"));
        }
        let mut config = SandboxConfig::linux(engine_path);
        config.workers = workers;
        Ok(Self {
            inner: Arc::new(EnginePool::new(config).map_err(error)?),
        })
    }

    /// Choose from a native environment's seat projection without changing its state.
    #[pyo3(signature = (env, seat, level=5, seed=0, time_ms=5000))]
    fn choose(
        &self,
        py: Python<'_>,
        env: &NativeEnv,
        seat: u8,
        level: u8,
        seed: u64,
        time_ms: u64,
    ) -> PyResult<u32> {
        py.detach(|| {
            let mut limits = SearchLimits::for_level(level, seed).map_err(error)?;
            limits.time_ms = time_ms;
            limits.validate().map_err(error)?;
            let settings = Settings {
                skill: Some([0, 2, 4, 6, 8, 10, 12, 14, 17, 20][usize::from(level - 1)]),
                ..Settings::default()
            };
            let player = UciOpponent::new(
                self.inner.clone(),
                gfa_games::registry().map_err(error)?.get(&env.game_id).map_err(error)?,
                env.config.clone(),
                settings,
            ).map_err(error)?;
            let observation = env.inner.observe(Viewer::Player(seat)).map_err(error)?;
            let actions = env.inner.action_catalog(seat).map_err(error)?;
            let result = player.decide(
                &PlayerTurn { seat, observation: &observation, legal_actions: &actions },
                limits,
                &gfa_opponents::SystemClock::default(),
            ).map_err(error)?;
            Ok(result.action.index)
        })
    }
}

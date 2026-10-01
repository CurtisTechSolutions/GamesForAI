//! Native Python training bridge. Typed engine states stay in Rust between steps.
mod vector;

use gfa_core::{EnvSnapshot, GameError, TrainingEnv, Viewer};
use numpy::{ndarray::IxDyn, PyArray1, PyArrayMethods};
use pyo3::{exceptions::PyValueError, prelude::*, types::PyDict};

fn error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

/// A native game with explicit acting seats; Gymnasium/AEC policies wrap this object.
#[pyclass(module = "gamesforai._native")]
struct NativeEnv {
    inner: Box<dyn TrainingEnv>,
    config: serde_json::Value,
    #[pyo3(get)]
    game_id: String,
    #[pyo3(get)]
    action_space_size: u32,
    #[pyo3(get)]
    num_players: usize,
}
#[pymethods]
impl NativeEnv {
    #[new]
    #[pyo3(signature = (game_id, config_json="{}", seed=0))]
    fn new(py: Python<'_>, game_id: &str, config_json: &str, seed: u64) -> PyResult<Self> {
        if config_json.len() > 64 * 1024 {
            return Err(error("configuration exceeds 64 KiB"));
        }
        let config = serde_json::from_str(config_json).map_err(error)?;
        let inner = py
            .detach(|| gfa_games::registry()?.training_env(game_id, &config, seed))
            .map_err(error)?;
        Ok(Self {
            action_space_size: inner.spec().action_space_size,
            num_players: inner.returns().len(),
            game_id: game_id.into(),
            config,
            inner,
        })
    }

    #[pyo3(signature = (seed=0, position=None))]
    fn reset(&mut self, py: Python<'_>, seed: u64, position: Option<&str>) -> PyResult<()> {
        py.detach(|| self.inner.reset(seed, position))
            .map_err(error)
    }

    fn step(&mut self, py: Python<'_>, seat: u8, action: u32) -> PyResult<(Vec<f64>, bool, bool)> {
        let step = py
            .detach(|| self.inner.step_index(seat, action))
            .map_err(error)?;
        Ok((step.rewards, step.terminated, step.truncated))
    }

    fn step_string(
        &mut self,
        py: Python<'_>,
        seat: u8,
        action: &str,
    ) -> PyResult<(Vec<f64>, bool, bool)> {
        let step = py
            .detach(|| self.inner.step_string(seat, action))
            .map_err(error)?;
        Ok((step.rewards, step.terminated, step.truncated))
    }

    fn builtin_action(
        &self,
        py: Python<'_>,
        seat: u8,
        algorithm: &str,
        level: u8,
        seed: u64,
    ) -> PyResult<u32> {
        use gfa_opponents::{Algorithm, Opponent, PlayerTurn, SearchLimits, SearchOpponent};
        // Fixed node/depth budgets make training independent of machine speed.
        struct NodeClock;
        impl gfa_core::Clock for NodeClock {
            fn now_ms(&self) -> u64 {
                0
            }
        }
        let algorithm = match algorithm {
            "minimax" => Algorithm::Minimax,
            "mcts" => Algorithm::Mcts,
            _ => return Err(error("builtin algorithm must be minimax or mcts")),
        };
        py.detach(|| {
            let limits = SearchLimits::for_level(level, seed)?;
            let player = SearchOpponent::new(
                gfa_games::registry()?.get(&self.game_id)?,
                self.config.clone(),
                algorithm,
            )?;
            let observation = self.inner.observe(Viewer::Player(seat))?;
            let actions = self.inner.action_catalog(seat)?;
            let choice = player.choose_action(
                &PlayerTurn {
                    seat,
                    observation: &observation,
                    legal_actions: &actions,
                },
                limits,
                &NodeClock,
            )?;
            Ok::<_, GameError>(choice.action.index)
        })
        .map_err(error)
    }

    fn spec_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner.spec()).map_err(error)
    }

    fn current_players(&self) -> Vec<usize> {
        self.inner
            .current_players()
            .into_iter()
            .map(usize::from)
            .collect()
    }

    fn flags(&self) -> (bool, bool) {
        (self.inner.terminated(), self.inner.truncated())
    }

    fn returns(&self) -> Vec<f64> {
        self.inner.returns()
    }

    fn frame<'py>(&self, py: Python<'py>, seat: u8) -> PyResult<Bound<'py, PyDict>> {
        let (observation, mask, actions) = py
            .detach(|| {
                Ok::<_, GameError>((
                    self.inner.observe(Viewer::Player(seat))?,
                    self.inner.action_mask(seat)?,
                    self.inner.action_catalog(seat)?,
                ))
            })
            .map_err(error)?;
        let tensor = observation
            .tensor
            .ok_or_else(|| error("game has no numeric observation"))?;
        let dict = PyDict::new(py);
        let array = PyArray1::from_vec(py, tensor.values).reshape(IxDyn(&tensor.shape))?;
        dict.set_item("observation", array)?;
        dict.set_item(
            "action_mask",
            PyArray1::from_vec(
                py,
                mask.into_iter()
                    .map(|legal| if legal { 1_i8 } else { 0 })
                    .collect(),
            ),
        )?;
        dict.set_item("text", observation.text)?;
        dict.set_item(
            "legal_actions",
            actions
                .into_iter()
                .map(|action| (action.index, action.string))
                .collect::<Vec<_>>(),
        )?;
        dict.set_item(
            "board_json",
            serde_json::to_string(&observation.json).map_err(error)?,
        )?;
        Ok(dict)
    }

    fn public_text(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| {
            self.inner
                .observe(Viewer::Spectator)
                .map(|observation| observation.text)
        })
        .map_err(error)
    }

    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            game_id: self.game_id.clone(),
            config: self.config.clone(),
            action_space_size: self.action_space_size,
            num_players: self.num_players,
        }
    }

    fn get_state(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| {
            let snapshot = self.inner.get_state()?;
            serde_json::to_string(&snapshot).map_err(GameError::from)
        })
        .map_err(error)
    }

    fn set_state(&mut self, py: Python<'_>, snapshot: &str) -> PyResult<()> {
        if snapshot.len() > 16 * 1024 * 1024 {
            return Err(error("checkpoint exceeds 16 MiB"));
        }
        py.detach(|| {
            let snapshot: EnvSnapshot = serde_json::from_str(snapshot)?;
            self.inner.set_state(&snapshot)
        })
        .map_err(error)
    }
}

/// Installed native games in stable identifier order.
#[pyfunction]
fn games() -> PyResult<Vec<String>> {
    Ok(gfa_games::registry()
        .map_err(error)?
        .specs()
        .into_iter()
        .map(|spec| spec.id)
        .collect())
}

/// Bundled, versioned position datasets; manifests pin the exact UTF-8 bytes.
#[pyfunction]
fn position_set_data(name: &str) -> PyResult<(String, String)> {
    let (manifest, data) = match name {
        "chess-endgames-basic@1" => (
            include_str!("../../../../positions/chess-endgames-basic@1.manifest.json"),
            include_str!("../../../../positions/chess-endgames-basic@1.jsonl"),
        ),
        "connect4-solved-positions@1" => (
            include_str!("../../../../positions/connect4-solved-positions@1.manifest.json"),
            include_str!("../../../../positions/connect4-solved-positions@1.jsonl"),
        ),
        _ => return Err(error("unknown position set; use a published name@version")),
    };
    Ok((manifest.into(), data.into()))
}

/// GamesForAI's native engine bridge.
#[pymodule]
mod _native {
    #[pymodule_export]
    use super::{games, position_set_data, vector::NativeVectorEnv, NativeEnv};
}

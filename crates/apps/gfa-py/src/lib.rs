//! Native Python training bridge. Typed engine states stay in Rust between steps.
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

    fn current_players(&self) -> Vec<u8> {
        self.inner.current_players()
    }

    fn flags(&self) -> (bool, bool) {
        (self.inner.terminated(), self.inner.truncated())
    }

    fn returns(&self) -> Vec<f64> {
        self.inner.returns()
    }

    fn frame<'py>(&self, py: Python<'py>, seat: u8) -> PyResult<Bound<'py, PyDict>> {
        let (observation, mask) = py
            .detach(|| {
                Ok::<_, GameError>((
                    self.inner.observe(Viewer::Player(seat))?,
                    self.inner.action_mask(seat)?,
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
            "board_json",
            serde_json::to_string(&observation.json).map_err(error)?,
        )?;
        Ok(dict)
    }

    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            game_id: self.game_id.clone(),
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

/// GamesForAI's native engine bridge.
#[pymodule]
mod _native {
    #[pymodule_export]
    use super::{games, NativeEnv};
}

//! Independent, bounded batches. All native work runs outside the interpreter.
use crate::error;
use gfa_core::{EnvSnapshot, GameError, TrainingEnv, Viewer};
use numpy::{ndarray::IxDyn, PyArray1, PyArrayMethods};
use pyo3::{prelude::*, types::PyDict};
use rayon::{prelude::*, ThreadPool, ThreadPoolBuilder};
use std::sync::Arc;

struct Frame {
    values: Vec<f32>,
    mask: Vec<i8>,
    seat: u8,
    to_act: i16,
    terminated: bool,
    truncated: bool,
}

/// Parallel sequential-game environments, with explicit terminal resets.
#[pyclass(module = "gamesforai._native")]
pub(crate) struct NativeVectorEnv {
    envs: Vec<Box<dyn TrainingEnv>>,
    pool: Arc<ThreadPool>,
    shape: Vec<usize>,
    #[pyo3(get)]
    n: usize,
    #[pyo3(get)]
    num_players: usize,
    #[pyo3(get)]
    action_space_size: usize,
}

impl NativeVectorEnv {
    fn frames(
        &self,
        envs: &[Box<dyn TrainingEnv>],
        seats: Option<&[u8]>,
    ) -> Result<Vec<Frame>, GameError> {
        self.pool.install(|| {
            envs.par_iter()
                .enumerate()
                .map(|(index, env)| {
                    let actors = env.current_players();
                    if actors.len() > 1 {
                        return Err(GameError::position("VectorEnv requires sequential turns"));
                    }
                    let seat = seats.map_or_else(
                        || actors.first().copied().unwrap_or(0),
                        |seats| seats[index],
                    );
                    let tensor = env
                        .observe(Viewer::Player(seat))?
                        .tensor
                        .ok_or_else(|| GameError::position("Game has no numeric observation"))?;
                    if tensor.shape != self.shape {
                        return Err(GameError::position(
                            "Observation shape changed within the batch",
                        ));
                    }
                    Ok(Frame {
                        values: tensor.values,
                        mask: env.action_mask(seat)?.into_iter().map(i8::from).collect(),
                        seat,
                        to_act: actors.first().map_or(-1, |seat| i16::from(*seat)),
                        terminated: env.terminated(),
                        truncated: env.truncated(),
                    })
                })
                .collect()
        })
    }

    fn arrays<'py>(
        &self,
        py: Python<'py>,
        frames: Vec<Frame>,
        rewards: Vec<f64>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let mut shape = vec![self.n];
        shape.extend(&self.shape);
        let dict = PyDict::new(py);
        dict.set_item(
            "terminated",
            PyArray1::from_vec(py, frames.iter().map(|frame| frame.terminated).collect()),
        )?;
        dict.set_item(
            "truncated",
            PyArray1::from_vec(py, frames.iter().map(|frame| frame.truncated).collect()),
        )?;
        dict.set_item(
            "seat",
            PyArray1::from_vec(
                py,
                frames.iter().map(|frame| i16::from(frame.seat)).collect(),
            ),
        )?;
        dict.set_item(
            "to_act",
            PyArray1::from_vec(py, frames.iter().map(|frame| frame.to_act).collect()),
        )?;
        let mut values = Vec::new();
        let mut masks = Vec::new();
        for frame in frames {
            values.extend(frame.values);
            masks.extend(frame.mask);
        }
        dict.set_item(
            "observation",
            PyArray1::from_vec(py, values).reshape(IxDyn(&shape))?,
        )?;
        dict.set_item(
            "action_mask",
            PyArray1::from_vec(py, masks).reshape([self.n, self.action_space_size])?,
        )?;
        dict.set_item(
            "rewards",
            PyArray1::from_vec(py, rewards).reshape([self.n, self.num_players])?,
        )?;
        Ok(dict)
    }
}

#[pymethods]
impl NativeVectorEnv {
    #[new]
    #[pyo3(signature = (game_id, n, config_json="{}", seed=0, threads=0))]
    fn new(
        py: Python<'_>,
        game_id: &str,
        n: usize,
        config_json: &str,
        seed: u64,
        threads: usize,
    ) -> PyResult<Self> {
        if !(1..=4096).contains(&n) || threads > 64 || config_json.len() > 64 * 1024 {
            return Err(error(
                "require 1..4096 environments, 0..64 threads, and <=64 KiB config",
            ));
        }
        seed.checked_add((n - 1) as u64)
            .ok_or_else(|| error("batch seed overflow"))?;
        let config = serde_json::from_str(config_json).map_err(error)?;
        let workers = if threads == 0 {
            std::thread::available_parallelism()
                .map_or(1, usize::from)
                .min(64)
                .min(n)
        } else {
            threads.min(n)
        };
        let pool = Arc::new(
            py.detach(|| ThreadPoolBuilder::new().num_threads(workers).build())
                .map_err(error)?,
        );
        let envs: Vec<Box<dyn TrainingEnv>> = py
            .detach(|| {
                pool.install(|| {
                    let registry = gfa_games::registry()?;
                    (0..n)
                        .into_par_iter()
                        .map(|index| registry.training_env(game_id, &config, seed + index as u64))
                        .collect::<Result<_, GameError>>()
                })
            })
            .map_err(error)?;
        let first = &envs[0];
        let tensor = py
            .detach(|| first.observe(Viewer::Player(0)))
            .map_err(error)?
            .tensor
            .ok_or_else(|| error("Game has no numeric observation"))?;
        let action_space_size = first.spec().action_space_size as usize;
        let num_players = first.returns().len();
        let row_bytes = tensor
            .values
            .len()
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(action_space_size))
            .and_then(|bytes| bytes.checked_add(num_players * 8 + 6))
            .ok_or_else(|| error("batch array size overflow"))?;
        if row_bytes
            .checked_mul(n)
            .is_none_or(|bytes| bytes > 128 * 1024 * 1024)
        {
            return Err(error("batch numeric arrays exceed 128 MiB"));
        }
        Ok(Self {
            envs,
            pool,
            shape: tensor.shape,
            n,
            num_players,
            action_space_size,
        })
    }

    #[pyo3(signature = (seats=None))]
    fn frame<'py>(&self, py: Python<'py>, seats: Option<Vec<u8>>) -> PyResult<Bound<'py, PyDict>> {
        if seats.as_ref().is_some_and(|seats| seats.len() != self.n) {
            return Err(error("seat count must equal batch size"));
        }
        let frames = py
            .detach(|| self.frames(&self.envs, seats.as_deref()))
            .map_err(error)?;
        self.arrays(py, frames, vec![0.0; self.n * self.num_players])
    }

    fn step<'py>(
        &mut self,
        py: Python<'py>,
        actions: Vec<Option<u32>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        if actions.len() != self.n {
            return Err(error("action count must equal batch size"));
        }
        let (candidate, frames, rewards) = py
            .detach(|| {
                let mut candidate = self.envs.clone();
                let transitions: Vec<Vec<f64>> = self.pool.install(|| {
                    candidate
                        .par_iter_mut()
                        .zip(actions)
                        .map(|(env, action)| {
                            let actors = env.current_players();
                            match (actors.as_slice(), action) {
                                ([], None) if env.terminated() || env.truncated() => {
                                    Ok(vec![0.0; self.num_players])
                                }
                                ([seat], Some(action)) => {
                                    Ok(env.step_index(*seat, action)?.rewards)
                                }
                                _ => Err(GameError::illegal(
                                    "Use a legal index for active rows and None for finished rows",
                                )),
                            }
                        })
                        .collect::<Result<_, GameError>>()
                })?;
                let frames = self.frames(&candidate, None)?;
                Ok::<_, GameError>((
                    candidate,
                    frames,
                    transitions.into_iter().flatten().collect(),
                ))
            })
            .map_err(error)?;
        let arrays = self.arrays(py, frames, rewards)?;
        self.envs = candidate;
        Ok(arrays)
    }

    fn reset_at<'py>(
        &mut self,
        py: Python<'py>,
        indices: Vec<usize>,
        seeds: Vec<u64>,
        positions: Vec<Option<String>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        if indices.len() != seeds.len() || indices.len() != positions.len() {
            return Err(error(
                "indices, seeds, and positions must have equal lengths",
            ));
        }
        let mut jobs = vec![None; self.n];
        for ((index, seed), position) in indices.into_iter().zip(seeds).zip(positions) {
            if index >= self.n || jobs[index].is_some() {
                return Err(error("reset indices must be unique and in range"));
            }
            if position
                .as_ref()
                .is_some_and(|position| position.len() > 16 * 1024 * 1024)
            {
                return Err(error("position exceeds 16 MiB"));
            }
            jobs[index] = Some((seed, position));
        }
        let (candidate, frames) = py
            .detach(|| {
                let mut candidate = self.envs.clone();
                self.pool.install(|| {
                    candidate
                        .par_iter_mut()
                        .zip(jobs)
                        .try_for_each(|(env, job)| {
                            if let Some((seed, position)) = job {
                                env.reset(seed, position.as_deref())?;
                            }
                            Ok::<_, GameError>(())
                        })
                })?;
                let frames = self.frames(&candidate, None)?;
                Ok::<_, GameError>((candidate, frames))
            })
            .map_err(error)?;
        let arrays = self.arrays(py, frames, vec![0.0; self.n * self.num_players])?;
        self.envs = candidate;
        Ok(arrays)
    }

    fn clone(&self) -> Self {
        Self {
            envs: self.envs.clone(),
            pool: self.pool.clone(),
            shape: self.shape.clone(),
            n: self.n,
            num_players: self.num_players,
            action_space_size: self.action_space_size,
        }
    }

    fn get_state(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        py.detach(|| {
            self.pool.install(|| {
                self.envs
                    .par_iter()
                    .map(|env| serde_json::to_string(&env.get_state()?).map_err(GameError::from))
                    .collect::<Result<_, GameError>>()
            })
        })
        .map_err(error)
    }

    fn set_state(&mut self, py: Python<'_>, snapshots: Vec<String>) -> PyResult<()> {
        if snapshots.len() != self.n
            || snapshots
                .iter()
                .any(|snapshot| snapshot.len() > 16 * 1024 * 1024)
            || snapshots.iter().map(String::len).sum::<usize>() > 128 * 1024 * 1024
        {
            return Err(error("checkpoint count or size is invalid"));
        }
        let candidate = py
            .detach(|| {
                let mut candidate = self.envs.clone();
                self.pool.install(|| {
                    candidate
                        .par_iter_mut()
                        .zip(snapshots)
                        .try_for_each(|(env, snapshot)| {
                            let snapshot: EnvSnapshot = serde_json::from_str(&snapshot)?;
                            env.set_state(&snapshot)
                        })
                })?;
                self.frames(&candidate, None)?;
                Ok::<_, GameError>(candidate)
            })
            .map_err(error)?;
        self.envs = candidate;
        Ok(())
    }
}

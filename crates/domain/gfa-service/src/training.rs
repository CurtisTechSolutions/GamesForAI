use crate::{error, GameService};
use gfa_api_types::{
    ApiError, TrainingBatch, TrainingBatchResult, TrainingFrame, TrainingOperation, TrainingResult,
};
use gfa_core::{EnvSnapshot, TrainingEnv, Viewer};

const MAX_OPERATIONS: usize = 64;
const MAX_ROW_BYTES: usize = 1024 * 1024;
const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;

fn limit(message: &str) -> ApiError {
    ApiError::new(
        "BATCH_LIMIT",
        message,
        "Split the batch into smaller requests.",
    )
}

impl GameService {
    /// Run stateless owner-only training work without reading or writing match storage.
    /// Each input is independent. Full checkpoints are never policy observations.
    pub fn training_batch(&self, request: TrainingBatch) -> Result<TrainingBatchResult, ApiError> {
        if request.operations.is_empty() || request.operations.len() > MAX_OPERATIONS {
            return Err(limit("Training batches require 1..64 operations"));
        }
        let count = request.operations.len();
        // Reserve envelope bytes and enough room for a bounded error in every later row.
        let mut remaining = MAX_RESULT_BYTES - 256;
        let mut results = Vec::with_capacity(count);
        for (index, operation) in request.operations.into_iter().enumerate() {
            let mut result = match self.training_operation(operation) {
                Ok(result) => result,
                Err(error) => TrainingResult::Error { error },
            };
            let bytes = serde_json::to_vec(&result).map_err(|error| error::engine(error.into()))?;
            if bytes.len() > MAX_ROW_BYTES
                || bytes.len() > remaining.saturating_sub((count - index - 1) * 512)
            {
                result = TrainingResult::Error {
                    error: limit("Training result exceeds the row or batch output bound"),
                };
            }
            remaining -= serde_json::to_vec(&result)
                .map_err(|error| error::engine(error.into()))?
                .len();
            results.push(result);
        }
        Ok(TrainingBatchResult { results })
    }

    fn restore_training(&self, checkpoint: &EnvSnapshot) -> Result<Box<dyn TrainingEnv>, ApiError> {
        let mut env = self
            .registry
            .training_env(&checkpoint.game_id, &checkpoint.config, 0)
            .map_err(error::engine)?;
        env.set_state(checkpoint).map_err(error::engine)?;
        Ok(env)
    }

    fn training_operation(&self, operation: TrainingOperation) -> Result<TrainingResult, ApiError> {
        let (env, seat, rewards) = match operation {
            TrainingOperation::Create {
                game_id,
                config,
                seed,
                position,
                seat,
            } => {
                let mut env = self
                    .registry
                    .training_env(&game_id, &config, seed)
                    .map_err(error::engine)?;
                if let Some(position) = position {
                    env.reset(seed, Some(&position)).map_err(error::engine)?;
                }
                (env, Some(seat), None)
            }
            TrainingOperation::Reset {
                checkpoint,
                seed,
                position,
                seat,
            } => {
                let mut env = self.restore_training(&checkpoint)?;
                env.reset(seed, position.as_deref())
                    .map_err(error::engine)?;
                (env, Some(seat), None)
            }
            TrainingOperation::Step {
                checkpoint,
                seat,
                action,
            } => {
                let mut env = self.restore_training(&checkpoint)?;
                let result = env.step_index(seat, action).map_err(error::engine)?;
                (env, Some(seat), Some(result.rewards))
            }
            TrainingOperation::Observe { checkpoint, seat } => {
                (self.restore_training(&checkpoint)?, seat, None)
            }
        };
        let returns = env.returns();
        let frame = TrainingFrame {
            seat,
            observation: env
                .observe(seat.map_or(Viewer::Spectator, Viewer::Player))
                .map_err(error::engine)?,
            legal_actions: match seat {
                Some(seat) => env.action_catalog(seat).map_err(error::engine)?,
                None => vec![],
            },
            action_mask: match seat {
                Some(seat) => env.action_mask(seat).map_err(error::engine)?,
                None => vec![false; env.spec().action_space_size as usize],
            },
            to_act: env.current_players(),
            rewards: rewards.unwrap_or_else(|| vec![0.0; returns.len()]),
            returns,
            terminated: env.terminated(),
            truncated: env.truncated(),
        };
        Ok(TrainingResult::Ok {
            checkpoint: env.get_state().map_err(error::engine)?,
            frame: Box::new(frame),
        })
    }
}

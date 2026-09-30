//! Runtime glue for the service's bounded opponent jobs.
use gfa_service::{OpponentExecutor, OpponentFuture, OpponentJob};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub(super) struct Workers(Arc<Semaphore>);

impl Workers {
    pub(super) fn new(capacity: usize) -> Self {
        Self(Arc::new(Semaphore::new(capacity)))
    }
}

impl OpponentExecutor for Workers {
    fn execute(&self, job: OpponentJob) -> OpponentFuture<'_> {
        let capacity = self.0.clone();
        Box::pin(async move {
            let permit = capacity.try_acquire_owned().map_err(|_| {
                gfa_api_types::ApiError::new("ENGINE_BUSY", "All opponent workers are busy", "Retry after a running search finishes.")
            })?;
            tokio::task::spawn_blocking(move || {
                // A dropped HTTP request must not release capacity while CPU work continues.
                let _permit = permit;
                job.run(&gfa_opponents::SystemClock::default())
            }).await.map_err(|_| gfa_api_types::ApiError::new("ENGINE_UNAVAILABLE", "Opponent worker failed", "Retry or select another opponent."))?
        })
    }
}

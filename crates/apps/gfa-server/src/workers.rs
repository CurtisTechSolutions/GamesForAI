//! Runtime glue for the service's bounded opponent jobs.
use gfa_service::{OpponentExecutor, OpponentFuture, OpponentJob};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::Semaphore;

pub(super) struct Workers(Arc<Semaphore>);

impl Workers {
    pub(super) fn new(capacity: usize) -> Self {
        Self(Arc::new(Semaphore::new(capacity)))
    }
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

struct SearchClock {
    cancelled: Arc<AtomicBool>,
    clock: gfa_opponents::SystemClock,
}

impl gfa_opponents::Clock for SearchClock {
    fn now_ms(&self) -> u64 {
        if self.cancelled.load(Ordering::Relaxed) {
            u64::MAX
        } else {
            gfa_opponents::Clock::now_ms(&self.clock)
        }
    }
}

impl Workers {
    fn schedule<T: Send + 'static>(
        &self,
        job: impl FnOnce(&SearchClock) -> Result<T, gfa_api_types::ApiError> + Send + 'static,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, gfa_api_types::ApiError>> + Send>> {
        let capacity = self.0.clone();
        Box::pin(async move {
            let permit = capacity.try_acquire_owned().map_err(|_| {
                gfa_api_types::ApiError::new(
                    "ENGINE_BUSY", "All opponent workers are busy", "Retry after a running search finishes.",
                )
            })?;
            let cancelled = Arc::new(AtomicBool::new(false));
            let _cancel_on_drop = CancelOnDrop(cancelled.clone());
            tokio::task::spawn_blocking(move || {
                // Capacity remains leased until CPU work responds to cancellation and exits.
                let _permit = permit;
                job(&SearchClock { cancelled, clock: gfa_opponents::SystemClock::default() })
            }).await.map_err(|_| gfa_api_types::ApiError::new(
                "ENGINE_UNAVAILABLE", "Opponent worker failed", "Retry or select another opponent.",
            ))?
        })
    }
}
impl OpponentExecutor for Workers {
    fn execute(&self, job: OpponentJob) -> OpponentFuture<'_> {
        self.schedule(move |clock| job.run(clock))
    }
    fn analyze(&self, job: OpponentJob) -> gfa_service::AnalysisFuture<'_> {
        self.schedule(move |clock| job.analyze(clock))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gfa_api_types::{ApiError, OpponentConfig, OpponentSpec};
    use gfa_core::{
        serde_json::{json, Value},
        DynGame,
    };
    use gfa_opponents::{ActionChoice, Clock, Opponent, PlayerTurn, SearchLimits};
    use gfa_service::{BuiltinOpponentFactory, GameService, OpponentFactory};
    use std::time::{Duration, Instant};

    struct SlowFactory(Arc<AtomicBool>);
    impl OpponentFactory for SlowFactory {
        fn catalog(&self, game: &dyn DynGame) -> Vec<OpponentSpec> {
            BuiltinOpponentFactory.catalog(game)
        }
        fn create(
            &self,
            _: Arc<dyn DynGame>,
            _: Value,
            _: &OpponentConfig,
            seed: u64,
        ) -> Result<(Arc<dyn Opponent>, SearchLimits), ApiError> {
            struct Slow(Arc<AtomicBool>);
            impl Opponent for Slow {
                fn choose_action(
                    &self,
                    turn: &PlayerTurn<'_>,
                    limits: SearchLimits,
                    clock: &dyn Clock,
                ) -> Result<ActionChoice, gfa_core::GameError> {
                    self.0.store(true, Ordering::SeqCst);
                    while clock.now_ms() < 5000 {
                        std::thread::yield_now();
                    }
                    gfa_opponents::Random.choose_action(turn, limits, clock)
                }
            }
            Ok((
                Arc::new(Slow(self.0.clone())),
                SearchLimits {
                    nodes: 10,
                    depth: 1,
                    time_ms: 5000,
                    seed,
                },
            ))
        }
    }

    #[tokio::test]
    async fn cancelling_a_request_stops_search_and_recovers_worker_capacity(
    ) -> Result<(), super::super::ServerError> {
        let store = Arc::new(gfa_store::SqliteMatchStore::in_memory().await?);
        let workers = Arc::new(Workers::new(1));
        let started = Arc::new(AtomicBool::new(false));
        let host = Arc::new(super::super::Host);
        let slow = Arc::new(
            GameService::new(
                gfa_games::registry()?,
                store.clone(),
                host.clone(),
                host.clone(),
            )
            .with_opponents(Arc::new(SlowFactory(started.clone())), workers.clone()),
        );
        let initial = slow
            .create_match(
                serde_json::from_value(json!({
                    "game_id":"tictactoe", "assists":{"allow_analysis":true}
                }))?,
                gfa_core::Viewer::Player(0),
            )
            .await?;
        let request: gfa_api_types::AnalysisRequest = serde_json::from_value(json!({
            "game_id":"tictactoe", "from":{"match_id":initial.match_id,"seat":0},"opponent":{"id":"random"}
        }))?;
        let pending_request = request.clone();
        let pending = tokio::spawn(async move {
            slow.analyze(pending_request, gfa_core::Viewer::Player(0))
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !started.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await?;
        pending.abort();
        let _ = pending.await;
        let fast = GameService::new(gfa_games::registry()?, store.clone(), host.clone(), host)
            .with_opponents(Arc::new(BuiltinOpponentFactory), workers);
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match fast
                .analyze(request.clone(), gfa_core::Viewer::Player(0))
                .await
            {
                Ok(_) => break,
                Err(error) if error.code == "ENGINE_BUSY" && Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        store.close().await;
        Ok(())
    }
}

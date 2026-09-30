//! Runtime scheduling glue; durable discovery and move policy live in the service.
use gfa_api_types::ApiError;
use gfa_service::GameService;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::watch, task::JoinSet};

pub(super) async fn run(service: Arc<GameService>, mut stopped: watch::Receiver<bool>) {
    let mut jobs = JoinSet::new();
    let mut active = HashMap::new();
    let mut queue = VecDeque::new();
    let mut cooldowns = HashMap::new();
    let mut cursor = None;
    let mut last_error = None;
    let mut admit_at = Instant::now();
    while !*stopped.borrow() {
        let now = Instant::now();
        cooldowns.retain(|_, until| *until > now);
        if queue.is_empty() && jobs.len() < 4 && now >= admit_at {
            let page = tokio::select! {
                _ = stopped.changed() => break,
                result = service.pending_opponents(cursor.as_deref(), 32) => result,
            };
            match page {
                Ok(page) => {
                    cursor = page.next;
                    queue.extend(page.matches);
                    for failure in page.unavailable {
                        report(
                            &mut last_error,
                            format!("Match {:?}: {}", failure.match_id, failure.error.code),
                        );
                    }
                }
                Err(error) => report(&mut last_error, error.code),
            }
        }
        while jobs.len() < 4 && Instant::now() >= admit_at {
            let Some(id) = queue.pop_front() else {
                break;
            };
            if active.values().any(|running| running == &id) || cooldowns.contains_key(&id) {
                continue;
            }
            let service = service.clone();
            let match_id = id.clone();
            let task = jobs.spawn(async move { service.advance_opponents(&match_id, 1).await });
            active.insert(task.id(), id);
        }
        tokio::select! {
            _ = stopped.changed() => break,
            Some(joined) = jobs.join_next_with_id(), if !jobs.is_empty() => {
                let (task_id, result) = match joined {
                    Ok(value) => value,
                    Err(error) => (error.id(), Err(ApiError::new("ENGINE_UNAVAILABLE", "Opponent task failed", "Retry."))),
                };
                if let Some(id) = active.remove(&task_id) {
                    if let Err(error) = result {
                        // A competing host that wins a CAS has advanced durable play.
                        if error.code != "STALE_TURN" {
                            admit_at = Instant::now() + Duration::from_millis(50);
                            cool_down(&mut cooldowns, id.clone());
                            report(&mut last_error, format!("Match {id:?}: {}", error.code));
                        }
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
    jobs.shutdown().await;
}

fn cool_down(cooldowns: &mut HashMap<String, Instant>, id: String) {
    if cooldowns.len() >= 128 {
        let oldest = cooldowns
            .iter()
            .min_by_key(|(_, until)| **until)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            cooldowns.remove(&oldest);
        }
    }
    cooldowns.insert(id, Instant::now() + Duration::from_secs(1));
}

fn report(last: &mut Option<String>, message: String) {
    if last.as_ref() != Some(&message) {
        eprintln!("Opponent runner: {message}");
        *last = Some(message);
    }
}

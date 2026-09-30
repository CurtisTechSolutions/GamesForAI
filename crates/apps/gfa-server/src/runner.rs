//! Runtime scheduling glue; durable discovery and move policy live in the service.
use gfa_service::GameService;
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::watch, task::JoinSet};

pub(super) async fn run(service: Arc<GameService>, mut stopped: watch::Receiver<bool>) {
    let mut jobs = JoinSet::new();
    let mut active = HashSet::new();
    let mut queue = VecDeque::new();
    let mut cursor = None;
    let mut last_error = None;
    while !*stopped.borrow() {
        if queue.is_empty() && jobs.len() < 4 {
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
        while jobs.len() < 4 {
            let Some(id) = queue.pop_front() else {
                break;
            };
            if !active.insert(id.clone()) {
                continue;
            }
            let service = service.clone();
            jobs.spawn(async move {
                let result = service.advance_opponents(&id, 1).await;
                (id, result)
            });
        }
        tokio::select! {
            _ = stopped.changed() => break,
            Some(joined) = jobs.join_next(), if !jobs.is_empty() => {
                match joined {
                    Ok((id, result)) => {
                        active.remove(&id);
                        if let Err(error) = result {
                            // CAS conflicts are normal when another host wins the turn.
                            if error.code != "STALE_TURN" {
                                report(&mut last_error, format!("Match {id:?}: {}", error.code));
                            }
                        }
                    }
                    Err(error) => report(&mut last_error, format!("Opponent task failed: {error}")),
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
    jobs.abort_all();
    while jobs.join_next().await.is_some() {}
}

fn report(last: &mut Option<String>, message: String) {
    if last.as_ref() != Some(&message) {
        eprintln!("Opponent runner: {message}");
        *last = Some(message);
    }
}

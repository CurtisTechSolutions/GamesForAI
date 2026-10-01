use crate::{play_types::StateOutput, McpServer};
use gfa_core::Viewer;
use gfa_service::GameService;
use rmcp::{
    model::{ClientConfig, ResourceUpdatedNotificationParam, SubscriptionFilter},
    service::{Peer, RequestContext, SubscriptionContext},
    ErrorData, RoleServer,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinHandle,
};

const MAX_URIS: usize = 8;
const POLL: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(3);
struct LegacyWatch {
    // The SDK returns clones of this connection's stable Arc. Pointer identity
    // distinguishes clients with identical names without trusting client IDs.
    identity: Arc<ClientConfig>,
    uri: String,
    task: JoinHandle<()>,
}
impl Drop for LegacyWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub(crate) struct Watches {
    capacity: Arc<Semaphore>,
    legacy: Mutex<Vec<LegacyWatch>>,
}
impl Default for Watches {
    fn default() -> Self {
        Self {
            capacity: Arc::new(Semaphore::new(16)),
            legacy: Mutex::new(Vec::new()),
        }
    }
}
fn state_id(uri: &str) -> Option<&str> {
    let id = uri.strip_prefix("gfa://matches/")?.strip_suffix("/state")?;
    (!id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)))
    .then_some(id)
}
fn unavailable() -> ErrorData {
    ErrorData::internal_error("Resource subscription unavailable", None)
}
async fn snapshot(service: &GameService, viewer: Viewer, uri: &str) -> Result<String, ErrorData> {
    let id = state_id(uri).ok_or_else(|| {
        ErrorData::invalid_params("Only gfa://matches/{match_id}/state can be watched", None)
    })?;
    let state = tokio::time::timeout(IO_TIMEOUT, service.get_state(id, viewer))
        .await
        .map_err(|_| unavailable())?
        .map_err(|error| ErrorData::invalid_params(error.message, None))?;
    let state = StateOutput::from_state(state, viewer).map_err(|_| unavailable())?;
    let encoded = serde_json::to_string(&state).map_err(|_| unavailable())?;
    if encoded.len() > 64 * 1024 {
        return Err(ErrorData::invalid_params(
            "State exceeds the live watch size limit",
            None,
        ));
    }
    Ok(encoded)
}
impl McpServer {
    pub(crate) fn accepted_watches(&self, requested: &SubscriptionFilter) -> SubscriptionFilter {
        let mut uris = requested
            .resource_subscriptions
            .as_ref()
            .into_iter()
            .flatten()
            .filter(|uri| state_id(uri).is_some())
            .take(MAX_URIS)
            .cloned()
            .collect::<Vec<_>>();
        uris.sort();
        uris.dedup();
        SubscriptionFilter::builder()
            .resource_subscriptions(uris)
            .build()
    }
    pub(crate) async fn listen_states(
        &self,
        context: SubscriptionContext,
    ) -> Result<(), ErrorData> {
        let _permit = self
            .watches
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| unavailable())?;
        let uris = context
            .accepted()
            .resource_subscriptions
            .clone()
            .unwrap_or_default();
        if uris.is_empty() {
            return Ok(());
        }
        let mut previous = vec![None; uris.len()];
        let mut tick = tokio::time::interval(POLL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _=context.cancelled()=>return Ok(()),
                _=tick.tick()=>{}
            }
            for (index, uri) in uris.iter().enumerate() {
                let current = tokio::select! {
                    _=context.cancelled()=>return Ok(()),
                    state=snapshot(&self.service,self.viewer,uri)=>state?,
                };
                if previous[index].as_ref() != Some(&current) {
                    tokio::select! {
                        _=context.cancelled()=>return Ok(()),
                        sent=tokio::time::timeout(IO_TIMEOUT,context.sink().notify_resource_updated(uri))=> {
                            sent.map_err(|_|unavailable())?.map_err(|_|unavailable())?;
                        }
                    }
                    previous[index] = Some(current);
                }
            }
        }
    }
    pub(crate) async fn subscribe_state(
        &self,
        uri: String,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let identity = context.peer.peer_info().ok_or_else(unavailable)?;
        let current = snapshot(&self.service, self.viewer, &uri).await?;
        let mut watches = self.watches.legacy.lock().await;
        watches.retain(|watch| !watch.task.is_finished());
        if watches
            .iter()
            .any(|watch| Arc::ptr_eq(&watch.identity, &identity) && watch.uri == uri)
        {
            return Ok(());
        }
        let permit = self
            .watches
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| unavailable())?;
        let service = self.service.clone();
        let viewer = self.viewer;
        let watched_uri = uri.clone();
        let task = tokio::spawn(async move {
            let _permit = permit;
            legacy_loop(service, viewer, watched_uri, context.peer, current).await;
        });
        watches.push(LegacyWatch {
            identity,
            uri,
            task,
        });
        Ok(())
    }
    pub(crate) async fn unsubscribe_state(
        &self,
        uri: &str,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let identity = context.peer.peer_info().ok_or_else(unavailable)?;
        self.watches.legacy.lock().await.retain(|watch| {
            !watch.task.is_finished()
                && !(watch.uri == uri && Arc::ptr_eq(&watch.identity, &identity))
        });
        Ok(())
    }
}
async fn legacy_loop(
    service: Arc<GameService>,
    viewer: Viewer,
    uri: String,
    peer: Peer<RoleServer>,
    mut previous: String,
) {
    loop {
        tokio::time::sleep(POLL).await;
        if peer.is_transport_closed() {
            return;
        }
        let Ok(current) = snapshot(&service, viewer, &uri).await else {
            return;
        };
        if current != previous {
            let sent = tokio::time::timeout(
                IO_TIMEOUT,
                peer.notify_resource_updated(ResourceUpdatedNotificationParam::new(&uri)),
            )
            .await;
            if !matches!(sent, Ok(Ok(()))) {
                return;
            }
            previous = current;
        }
    }
}

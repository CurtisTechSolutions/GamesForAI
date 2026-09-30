//! Local server composition: registry, lifecycle service, SQLite, and HTTP.
use gfa_service::{Clock, GameService, MatchIds};
use gfa_store::SqliteMatchStore;
use std::{
    error::Error,
    future::Future,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::net::TcpListener;
use uuid::Uuid;

/// Host startup or serving failure.
pub type ServerError = Box<dyn Error + Send + Sync>;

/// Configuration for auth-disabled, single-user local play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Database file; its parent directory must already exist.
    pub sqlite: PathBuf,
    /// Loopback port. Zero lets the OS select an available port.
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sqlite: "gfa.sqlite".into(),
            port: 8080,
        }
    }
}

struct Host;
impl Clock for Host {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or_default()
    }
}
impl MatchIds for Host {
    fn next_id(&self) -> String {
        format!("m_{}", Uuid::new_v4().simple())
    }
    fn next_seed(&self) -> u64 {
        // Fold independent UUID bits; neither half's fixed version/variant bits
        // constrain the resulting game seed.
        let entropy = Uuid::new_v4().as_u128();
        entropy as u64 ^ (entropy >> 64) as u64
    }
}

struct Application {
    router: axum::Router,
    store: Arc<SqliteMatchStore>,
    updates: Arc<gfa_http::LiveUpdates>,
    #[cfg(test)]
    service: Arc<GameService>,
}

async fn application(config: &Config, address: SocketAddr) -> Result<Application, ServerError> {
    let registry = gfa_games::registry()?;
    let store = Arc::new(SqliteMatchStore::open(&config.sqlite).await?);
    let host = Arc::new(Host);
    let updates = Arc::new(gfa_http::LiveUpdates::default());
    let service = Arc::new(
        GameService::new(registry, store.clone(), host.clone(), host)
            .with_observer(updates.clone()),
    );
    let router = gfa_http::local_router_with_updates(service.clone(), address, updates.clone())?;
    Ok(Application {
        router,
        store,
        updates,
        #[cfg(test)]
        service,
    })
}

/// Bind loopback, migrate the database, and serve until shutdown completes.
///
/// The address is printed after startup succeeds. A graceful shutdown drains HTTP
/// requests before closing SQLite. This local mode has no agent authentication.
pub async fn serve(
    config: Config,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServerError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port)).await?;
    let address = listener.local_addr()?;
    let app = application(&config, address).await?;
    println!("GamesForAI listening on http://{address} (local mode)");
    serve_application(listener, app, shutdown).await
}

async fn serve_application(
    listener: TcpListener,
    app: Application,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServerError> {
    let updates = app.updates.clone();
    let result = axum::serve(
        listener,
        app.router
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        updates.close();
    })
    .await;
    app.updates.wait_closed().await;
    app.store.close().await;
    result?;
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod stream_tests;

#[cfg(test)]
mod briefing_tests;

#[cfg(test)]
mod openapi_tests;

#[cfg(test)]
mod opponent_tests;

#[cfg(test)]
mod simulation_tests;

#[cfg(test)]
mod fork_tests;

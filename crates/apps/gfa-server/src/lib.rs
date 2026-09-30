//! Local server composition: registry, lifecycle service, persistence, and HTTP.
mod runner;
mod workers;

use gfa_service::{Clock, GameService, MatchIds, MatchStore};
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
    /// Persistent storage backend.
    pub database: Database,
    /// Loopback port. Zero lets the OS select an available port.
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: Database::Sqlite("gfa.sqlite".into()),
            port: 8080,
        }
    }
}

/// Storage backend selected at startup.
#[derive(Clone, PartialEq, Eq)]
pub enum Database {
    /// SQLite file; its parent directory must already exist.
    Sqlite(PathBuf),
    /// PostgreSQL URL. Debug output redacts credentials and connection details.
    #[cfg(feature = "postgres")]
    Postgres(String),
}

impl std::fmt::Debug for Database {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(path) => formatter.debug_tuple("Sqlite").field(path).finish(),
            #[cfg(feature = "postgres")]
            Self::Postgres(_) => formatter.write_str("Postgres([redacted])"),
        }
    }
}

enum Store {
    Sqlite(Arc<SqliteMatchStore>),
    #[cfg(feature = "postgres")]
    Postgres(Arc<gfa_store::PostgresMatchStore>),
}

impl Store {
    async fn open(database: &Database) -> Result<Self, ServerError> {
        match database {
            Database::Sqlite(path) => {
                Ok(Self::Sqlite(Arc::new(SqliteMatchStore::open(path).await?)))
            }
            #[cfg(feature = "postgres")]
            Database::Postgres(url) => Ok(Self::Postgres(Arc::new(
                gfa_store::PostgresMatchStore::connect(url).await?,
            ))),
        }
    }

    fn port(&self) -> Arc<dyn MatchStore> {
        match self {
            Self::Sqlite(store) => store.clone(),
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.clone(),
        }
    }

    async fn close(&self) {
        match self {
            Self::Sqlite(store) => store.close().await,
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.close().await,
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
    store: Store,
    updates: Arc<gfa_http::LiveUpdates>,
    service: Arc<GameService>,
}

async fn application(config: &Config, address: SocketAddr) -> Result<Application, ServerError> {
    let registry = gfa_games::registry()?;
    let store = Store::open(&config.database).await?;
    let host = Arc::new(Host);
    let updates = Arc::new(gfa_http::LiveUpdates::default());
    let service = Arc::new(
        GameService::new(registry, store.port(), host.clone(), host)
            .with_observer(updates.clone())
            .with_opponents(
                Arc::new(gfa_service::BuiltinOpponentFactory),
                Arc::new(workers::Workers::new(4)),
            ),
    );
    let router = gfa_http::local_router_with_updates(service.clone(), address, updates.clone())?;
    Ok(Application {
        router,
        store,
        updates,
        service,
    })
}

/// Bind loopback, migrate the database, and serve until shutdown completes.
///
/// The address is printed after startup succeeds. A graceful shutdown drains HTTP
/// requests before closing the database pool. This local mode has no agent authentication.
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
    let (stop_runner, stopped) = tokio::sync::watch::channel(false);
    let runner_task = tokio::spawn(runner::run(app.service.clone(), stopped));
    let stop_on_shutdown = stop_runner.clone();
    let result = axum::serve(
        listener,
        app.router
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        let _ = stop_on_shutdown.send(true);
        updates.close();
    })
    .await;
    let _ = stop_runner.send(true);
    let runner_result = runner_task.await;
    app.updates.wait_closed().await;
    app.store.close().await;
    runner_result?;
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

#[cfg(test)]
mod sudoku_tests;

#[cfg(test)]
mod analysis_tests;

#[cfg(test)]
mod seat_tests;

#[cfg(test)]
mod runner_tests;

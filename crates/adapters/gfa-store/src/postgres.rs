//! PostgreSQL persistence with row-level write locks and repeatable-read snapshots.
use gfa_service::{
    AppendResult, MatchEvent, MatchRecord, MatchStore, StoreError, StoreFuture, StoredCommand,
};
use sqlx::{postgres::PgPoolOptions, PgPool, Postgres, Transaction};
use std::time::Duration;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/postgres");

/// Shared connection pool backed by PostgreSQL with embedded, versioned migrations.
#[derive(Clone)]
pub struct PostgresMatchStore {
    pool: PgPool,
}

impl PostgresMatchStore {
    /// Connect to PostgreSQL and apply embedded migrations.
    ///
    /// URLs may select TLS with sslmode; the postgres feature enables Rustls and
    /// native trust roots. Callers should require verify-full for remote hosts.
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
            .map_err(unavailable)?;
        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(unavailable(error));
        }
        Ok(Self { pool })
    }

    /// Wait for pending operations and close all connections shared by this pool.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn insert(&self, record: MatchRecord) -> Result<(), StoreError> {
        let revision = revision(record.events.len())?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(unavailable)?;
        let result = sqlx::query!(
            "INSERT INTO gfa_matches (id, revision) VALUES ($1, $2)",
            record.id,
            revision
        )
        .execute(&mut *tx)
        .await;
        match result {
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                return Err(StoreError::DuplicateId)
            }
            Err(error) => return Err(unavailable(error)),
            Ok(_) => {}
        }
        insert_events(&mut tx, &record.id, 0, &record.events).await?;
        for command in &record.commands {
            insert_command(&mut tx, &record.id, command).await?;
        }
        tx.commit().await.map_err(unavailable)
    }

    async fn snapshot(&self, id: &str) -> Result<Option<MatchRecord>, StoreError> {
        let mut tx = self.pool
            .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .await.map_err(unavailable)?;
        let revision: Option<i64> =
            sqlx::query_scalar!("SELECT revision FROM gfa_matches WHERE id = $1", id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(unavailable)?;
        let Some(revision) = revision else {
            return Ok(None);
        };
        let rows = sqlx::query!(
            "SELECT sequence, payload FROM gfa_events WHERE match_id = $1 ORDER BY sequence",
            id
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(unavailable)?;
        if i64::try_from(rows.len()).ok() != Some(revision) {
            return Err(unavailable(
                "Event count does not match the stored revision",
            ));
        }
        let mut events = Vec::with_capacity(rows.len());
        for (expected, row) in rows.into_iter().enumerate() {
            if i64::try_from(expected).ok() != Some(row.sequence) {
                return Err(unavailable("Event sequence contains a gap"));
            }
            events.push(serde_json::from_str(&row.payload).map_err(unavailable)?);
        }
        let rows = sqlx::query!(
            "SELECT command_key, payload FROM gfa_commands WHERE match_id = $1 ORDER BY receipt_sequence",
            id
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(unavailable)?;
        let mut commands = Vec::with_capacity(rows.len());
        for row in rows {
            let command: StoredCommand = serde_json::from_str(&row.payload).map_err(unavailable)?;
            if command.key != row.command_key {
                return Err(unavailable("Command receipt key mismatch"));
            }
            commands.push(command);
        }
        tx.commit().await.map_err(unavailable)?;
        Ok(Some(MatchRecord {
            id: id.into(),
            events,
            commands,
        }))
    }

    async fn compare_and_append(
        &self,
        id: &str,
        expected: u64,
        events: Vec<MatchEvent>,
        command: Option<StoredCommand>,
    ) -> Result<AppendResult, StoreError> {
        // Lock this match's revision row before checking receipts or appending events.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(unavailable)?;
        let current: Option<i64> =
            sqlx::query_scalar!("SELECT revision FROM gfa_matches WHERE id = $1 FOR UPDATE", id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(unavailable)?;
        let current = current.ok_or(StoreError::NotFound)?;
        if let Some(command) = &command {
            let previous: Option<String> = sqlx::query_scalar!(
                "SELECT payload FROM gfa_commands WHERE match_id = $1 AND command_key = $2",
                id,
                command.key
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(unavailable)?;
            if let Some(previous) = previous {
                let previous: StoredCommand =
                    serde_json::from_str(&previous).map_err(unavailable)?;
                if previous.key != command.key {
                    return Err(unavailable("Command receipt key mismatch"));
                }
                return if previous.request == command.request {
                    Ok(AppendResult::AlreadyCommitted(Box::new(previous.response)))
                } else {
                    Err(StoreError::IdempotencyConflict)
                };
            }
        }
        if u64::try_from(current).ok() != Some(expected) {
            return Err(StoreError::Conflict);
        }
        let next = current
            .checked_add(revision(events.len())?)
            .ok_or_else(|| unavailable("Event revision overflow"))?;
        insert_events(&mut tx, id, current, &events).await?;
        if let Some(command) = &command {
            insert_command(&mut tx, id, command).await?;
        }
        sqlx::query!("UPDATE gfa_matches SET revision = $1 WHERE id = $2", next, id)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
        tx.commit().await.map_err(unavailable)?;
        Ok(AppendResult::Appended)
    }
}

impl MatchStore for PostgresMatchStore {
    fn create(&self, record: MatchRecord) -> StoreFuture<'_, ()> {
        Box::pin(self.insert(record))
    }

    fn load<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Option<MatchRecord>> {
        Box::pin(self.snapshot(id))
    }

    fn list_ids<'a>(&'a self, after: &'a str, limit: u32) -> StoreFuture<'a, Vec<String>> {
        Box::pin(async move {
            if !(1..=101).contains(&limit) {
                return Err(unavailable("History limit must be 1..101"));
            }
            let limit = i64::from(limit);
            sqlx::query_scalar!(
                "SELECT id FROM gfa_matches WHERE id > $1 ORDER BY id LIMIT $2",
                after,
                limit
            )
            .fetch_all(&self.pool)
            .await
            .map_err(unavailable)
        })
    }

    fn record_simulation<'a>(&'a self, id: &'a str, seat: u8, moves: u32) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let seat = i64::from(seat);
            let moves = i64::from(moves);
            sqlx::query!(
                "INSERT INTO gfa_assist_usage (match_id, seat, simulation_calls, simulated_moves) VALUES ($1, $2, 1, $3) ON CONFLICT(match_id, seat) DO UPDATE SET simulation_calls = gfa_assist_usage.simulation_calls + 1, simulated_moves = gfa_assist_usage.simulated_moves + excluded.simulated_moves",
                id,
                seat,
                moves
            ).execute(&self.pool).await.map_err(unavailable)?;
            Ok(())
        })
    }

    fn assist_usage<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Vec<gfa_api_types::AssistUsage>> {
        Box::pin(async move {
            let rows = sqlx::query!(
                "SELECT seat, simulation_calls, simulated_moves FROM gfa_assist_usage WHERE match_id = $1 ORDER BY seat",
                id
            ).fetch_all(&self.pool).await.map_err(unavailable)?;
            rows.into_iter()
                .map(|row| {
                    Ok(gfa_api_types::AssistUsage {
                        seat: u8::try_from(row.seat).map_err(unavailable)?,
                        simulation_calls: u64::try_from(row.simulation_calls)
                            .map_err(unavailable)?,
                        simulated_moves: u64::try_from(row.simulated_moves).map_err(unavailable)?,
                    })
                })
                .collect()
        })
    }

    fn append<'a>(
        &'a self,
        id: &'a str,
        expected_revision: u64,
        events: Vec<MatchEvent>,
        command: Option<StoredCommand>,
    ) -> StoreFuture<'a, AppendResult> {
        Box::pin(self.compare_and_append(id, expected_revision, events, command))
    }
}

async fn insert_events(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    start: i64,
    events: &[MatchEvent],
) -> Result<(), StoreError> {
    for (offset, event) in events.iter().enumerate() {
        let sequence = start
            .checked_add(revision(offset)?)
            .ok_or_else(|| unavailable("Event revision overflow"))?;
        let payload = serde_json::to_string(event).map_err(unavailable)?;
        sqlx::query!(
            "INSERT INTO gfa_events (match_id, sequence, payload) VALUES ($1, $2, $3)",
            id,
            sequence,
            payload
        )
        .execute(&mut **tx)
        .await
        .map_err(unavailable)?;
    }
    Ok(())
}

async fn insert_command(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    command: &StoredCommand,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(command).map_err(unavailable)?;
    sqlx::query!(
        "INSERT INTO gfa_commands (match_id, command_key, payload) VALUES ($1, $2, $3)",
        id,
        command.key,
        payload
    )
    .execute(&mut **tx)
    .await
    .map_err(unavailable)?;
    Ok(())
}

fn revision(length: usize) -> Result<i64, StoreError> {
    i64::try_from(length).map_err(unavailable)
}

fn unavailable(error: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(error.to_string())
}

#[cfg(test)]
#[path = "postgres_tests.rs"]
mod tests;

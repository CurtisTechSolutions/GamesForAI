use crate::{AppendResult, MatchEvent, MatchRecord, StoredCommand};
use std::{future::Future, pin::Pin};

/// Storage future independent of any async runtime.
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;

/// Storage failures that the service maps to stable API codes.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// Optimistic concurrency precondition did not match.
    #[error("event revision changed")]
    Conflict,
    /// The match does not exist.
    #[error("match not found")]
    NotFound,
    /// A generated match identifier already exists.
    #[error("match identifier already exists")]
    DuplicateId,
    /// The key already represents different request contents.
    #[error("idempotency key reused with different input")]
    IdempotencyConflict,
    /// Adapter failure; internal detail must not be returned to clients.
    #[error("storage unavailable: {0}")]
    Unavailable(String),
}

/// Persistence boundary. Implementations must make every write atomic.
pub trait MatchStore: Send + Sync {
    /// Insert a new record, rejecting an existing id without changing it.
    fn create(&self, record: MatchRecord) -> StoreFuture<'_, ()>;

    /// Read one consistent event/command snapshot.
    fn load<'a>(&'a self, id: &'a str) -> StoreFuture<'a, Option<MatchRecord>>;

    /// Compare-and-append using the current event count as the revision.
    ///
    /// Check idempotency before the revision: an identical command returns its
    /// original response, and key reuse with different input fails. Otherwise,
    /// compare expected_revision and atomically append every event plus the command.
    /// A failed operation must persist neither events nor command receipts.
    fn append<'a>(
        &'a self,
        id: &'a str,
        expected_revision: u64,
        events: Vec<MatchEvent>,
        command: Option<StoredCommand>,
    ) -> StoreFuture<'a, AppendResult>;
}

/// Host-supplied time, replaceable by a fake clock in tests.
pub trait Clock: Send + Sync {
    /// Unix milliseconds.
    fn now_ms(&self) -> u64;
}

/// Host-supplied identifiers and entropy; no global state is used by the service.
pub trait MatchIds: Send + Sync {
    /// Fresh, nonempty identifier.
    fn next_id(&self) -> String;
    /// Fresh seed when the caller did not request a fixed one.
    fn next_seed(&self) -> u64;
}

/// Best-effort notification after durable match commits.
///
/// Implementations must return promptly and must not panic. Notifications contain
/// only the match ID; consumers obtain authorized state from the service. Delivery
/// is not durable, so consumers must recover gaps from persisted events.
pub trait MatchObserver: Send + Sync {
    /// A creation or action batch has committed successfully.
    fn committed(&self, match_id: &str);
}

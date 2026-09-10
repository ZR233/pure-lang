//! Independent session persistence. Runtime memory is authoritative; SQLite follows asynchronously.

mod history;
mod repository;
mod sqlite;
mod thread_journal;
mod writer;

/// Current SQLite cold-history format. Unsupported formats are never automatically reset.
pub const SESSION_SCHEMA_VERSION: i64 = 6;

use std::sync::Arc;

use crate::storage::SessionEntryChange;
pub use sqlite::{SessionStoreError, SqliteSessionOptions};
pub use writer::SqliteSessionStore;

/// Observable writer state. Errors retain their typed cause; pending facts remain owned in memory.
#[derive(Debug, Clone)]
pub struct SessionPersistenceSnapshot {
    pub pending_commits: usize,
    pub admitted: u64,
    pub durable: u64,
    pub error: Option<Arc<SessionStoreError>>,
    pub stopped: bool,
}

/// Maximum metadata payload size; Thread journal entries use queue-pressure limits instead.
pub const DEFAULT_RESOURCE_MAX_BYTES: usize = 1024 * 1024;

/// Immutable metadata admission failures, before asynchronous storage begins.
#[derive(Debug, thiserror::Error)]
pub enum ResourceAdmissionError {
    #[error("invalid immutable resource identity: {0}")]
    InvalidIdentity(String),
    #[error("resource {id} revision conflict: expected {expected:?}, actual {actual:?}")]
    Conflict {
        id: String,
        expected: Option<u64>,
        actual: Option<u64>,
    },
    #[error("resource payload exceeds {limit} bytes")]
    TooLarge { limit: usize },
    #[error("resource admission sequence exhausted")]
    RevisionExhausted,
    #[error("resource store is stopping or closed")]
    StoreClosed,
}

//! Independent session persistence. Runtime memory is authoritative; SQLite follows asynchronously.

mod repository;
mod sqlite;
mod writer;

use std::sync::Arc;

pub use sea_orm::DbErr as SessionDatabaseError;
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

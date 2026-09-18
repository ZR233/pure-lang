use sea_orm::DatabaseConnection;
use std::path::PathBuf;

mod agent_framework;
pub(super) mod attachment;
pub(in crate::studio) mod directory;
mod error;
mod interaction;
pub(in crate::studio) mod object;
mod project;
pub(in crate::studio) mod settings;
pub(in crate::studio) mod ssh_migration;
mod thread;

#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
    sessions: pl_core::persistence::SqliteSessionStore,
    attachments_dir: PathBuf,
}

pub use error::StudioDatabaseError;
impl StudioStore {
    pub(crate) fn sessions(&self) -> &pl_core::persistence::SqliteSessionStore {
        &self.sessions
    }
    pub(crate) fn database(&self) -> &DatabaseConnection {
        &self.db
    }

    pub(crate) fn attachments_dir(&self) -> &std::path::Path {
        &self.attachments_dir
    }
}

// Legacy Task persistence tests were removed with the fixed Task runtime.

use sea_orm::DatabaseConnection;
use std::path::PathBuf;

mod agent_framework;
pub(super) mod attachment;
pub(in crate::studio) mod conversation_recovery;
mod error;
pub(in crate::studio) mod history;
mod interaction;
mod project;
mod settings;
mod task;
mod thread;

#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
    attachments_dir: PathBuf,
}

pub(in crate::studio) use agent_framework::UnregisteredThreadFault;
pub use error::StudioDatabaseError;
pub(in crate::studio) use thread::ChildThreadSpec;
impl StudioStore {
    pub(crate) fn database(&self) -> &DatabaseConnection {
        &self.db
    }

    pub(crate) fn attachments_dir(&self) -> &std::path::Path {
        &self.attachments_dir
    }
}

#[cfg(test)]
mod unit_tests;

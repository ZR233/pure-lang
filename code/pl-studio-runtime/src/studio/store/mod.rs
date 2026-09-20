use sea_orm::DatabaseConnection;
use std::path::PathBuf;
use std::sync::Arc;

use crate::studio::session_store::SessionStores;
use crate::studio::workspace_declarations::WorkspaceDeclarations;

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
    sessions: SessionStores,
    attachments_dir: PathBuf,
    workspaces: Arc<WorkspaceDeclarations>,
}

pub use error::StudioDatabaseError;
impl StudioStore {
    pub(crate) fn sessions(&self) -> &SessionStores {
        &self.sessions
    }
    pub(in crate::studio) fn workspaces(&self) -> &WorkspaceDeclarations {
        &self.workspaces
    }
    pub(crate) fn database(&self) -> &DatabaseConnection {
        &self.db
    }

    pub(crate) fn attachments_dir(&self) -> &std::path::Path {
        &self.attachments_dir
    }
}

// Legacy Task persistence tests were removed with the fixed Task runtime.

//! Typed SSH management commands delegated to `pl-core::remote`.

use anyhow::Result;
use pl_core::remote::{SshConnectionSnapshot, SshServerProfile};
use pl_protocol::remote::RemoteDirectoryListing;

use crate::studio::records::ProjectRecord;
use crate::studio::store::directory::{DirectoryDelta, ProjectDirectoryRecord};

use super::StudioRuntime;

impl StudioRuntime {
    pub(super) async fn hydrate_ssh_servers(&self) -> Result<()> {
        for profile in self.store.list_ssh_servers().await? {
            self.ssh_manager.save_server(profile).await?;
        }
        Ok(())
    }

    pub async fn list_ssh_servers(&self) -> Result<Vec<SshServerProfile>> {
        Ok(self.ssh_manager.list_servers().await)
    }

    pub async fn save_ssh_server(
        &self,
        mut profile: SshServerProfile,
        password: Option<String>,
    ) -> Result<SshServerProfile> {
        if profile.id.trim().is_empty() {
            profile.id = crate::studio::ids::new_id("ssh-server");
        }
        let previous = self
            .ssh_manager
            .list_servers()
            .await
            .into_iter()
            .find(|server| server.id == profile.id);
        let profile = self.ssh_manager.save_server(profile).await?;
        if let Err(error) = self.store.save_ssh_server(&profile).await {
            if let Some(previous) = previous {
                self.ssh_manager.save_server(previous).await?;
            } else {
                self.ssh_manager.delete_server(&profile.id).await?;
            }
            return Err(error);
        }
        if let Some(password) = password {
            self.ssh_manager.lease_password(&profile.id, password).await;
        }
        Ok(profile)
    }

    pub async fn delete_ssh_server(&self, server_id: &str) -> Result<()> {
        self.store.delete_ssh_server(server_id).await?;
        self.ssh_manager.delete_server(server_id).await?;
        Ok(())
    }

    pub async fn test_ssh_connection(&self, server_id: &str) -> Result<SshConnectionSnapshot> {
        Ok(self.ssh_manager.test_connection(server_id).await?)
    }

    pub async fn reconnect_ssh_server(&self, server_id: &str) -> Result<SshConnectionSnapshot> {
        self.ssh_manager.reconnect_server(server_id).await?;
        Ok(self.ssh_manager.connection_snapshot(server_id).await?)
    }

    pub async fn browse_remote_directories(
        &self,
        server_id: &str,
        path: Option<String>,
    ) -> Result<RemoteDirectoryListing> {
        Ok(self.ssh_manager.browse_directories(server_id, path).await?)
    }

    pub async fn open_remote_project(
        &self,
        server_id: &str,
        path: String,
    ) -> Result<ProjectRecord> {
        let workspace = self.ssh_manager.open_workspace(server_id, path).await?;
        let canonical_path = workspace.canonical_path().to_string();
        let name = canonical_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("remote-workspace")
            .to_string();
        let now = crate::studio::unix_seconds();
        let existing = self
            .store
            .find_project_by_path(&canonical_path, Some(server_id))
            .await?;
        let (id, created_at) = existing
            .map(|row| (row.id, row.created_at))
            .unwrap_or_else(|| (crate::studio::ids::new_id("project"), now));
        let delta = ProjectDirectoryRecord {
            id: id.clone(),
            name: name.clone(),
            path: canonical_path.clone(),
            ssh_server_id: Some(server_id.to_string()),
            created_at,
            updated_at: now,
            last_opened_at: Some(now),
            closed: false,
        };
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta::upsert_project(delta))
            .await?;
        Ok(ProjectRecord {
            id,
            name,
            path: canonical_path,
            ssh_server_id: Some(server_id.to_string()),
            updated_at: now,
        })
    }
}

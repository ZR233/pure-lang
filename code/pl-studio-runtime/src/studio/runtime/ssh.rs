//! Typed SSH management commands delegated to `pl-tool::remote`.

use anyhow::Result;
use pl_protocol::remote::RemoteDirectoryListing;
use pl_tool::remote::{SshConfigEntry, SshConnectionSnapshot, SshServerProfile};

use crate::studio::records::ProjectRecord;
use crate::studio::store::directory::{DirectoryDelta, ProjectDirectoryRecord};

use super::StudioRuntime;

impl StudioRuntime {
    pub(super) async fn hydrate_ssh_servers(&self) -> Result<()> {
        self.ssh_manager
            .reload_servers()
            .await
            .map_err(|error| anyhow::anyhow!(error))
    }

    pub async fn list_ssh_servers(&self) -> Result<Vec<SshConfigEntry>> {
        // 以文件为唯一事实源；每次列表都重新解析，拾取手工编辑。
        self.hydrate_ssh_servers().await?;
        Ok(self.ssh_manager.list_servers().await)
    }

    pub async fn save_ssh_server(&self, profile: SshServerProfile) -> Result<SshServerProfile> {
        let previous = self
            .ssh_manager
            .list_servers()
            .await
            .into_iter()
            .find(|server| server.profile.alias == profile.alias)
            .map(|server| server.profile);
        let profile = self
            .ssh_manager
            .save_server(profile)
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        if previous.as_ref() != Some(&profile) {
            self.thread_factory.invalidate_ssh_bindings(&profile.alias);
            self.tool_catalog_updates.notify_one();
        }
        Ok(profile)
    }

    pub async fn delete_ssh_server(&self, alias: &str) -> Result<()> {
        self.ssh_manager
            .delete_server(alias)
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        self.thread_factory.invalidate_ssh_bindings(alias);
        self.tool_catalog_updates.notify_one();
        Ok(())
    }

    pub async fn test_ssh_connection(&self, alias: &str) -> Result<SshConnectionSnapshot> {
        self.ssh_manager
            .test_connection(alias)
            .await
            .map_err(|error| anyhow::anyhow!(error))
    }

    pub async fn reconnect_ssh_server(&self, alias: &str) -> Result<SshConnectionSnapshot> {
        self.ssh_manager
            .reconnect_server(alias)
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(self.ssh_manager.connection_snapshot(alias).await?)
    }

    pub async fn browse_remote_directories(
        &self,
        alias: &str,
        path: Option<String>,
    ) -> Result<RemoteDirectoryListing> {
        self.ssh_manager
            .browse_directories(alias, path)
            .await
            .map_err(|error| anyhow::anyhow!(error))
    }

    pub async fn open_remote_project(&self, alias: &str, path: String) -> Result<ProjectRecord> {
        let workspace = self
            .ssh_manager
            .open_workspace(alias, path)
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let canonical_path = workspace.canonical_path().to_string();
        let _guard = self.lifecycle_lock.lock().await;
        if let Some(project) = self
            .agent_facility
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| {
                project.path == canonical_path && project.ssh_alias.as_deref() == Some(alias)
            })
        {
            return Ok(project);
        }
        let derived_name = canonical_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("remote-workspace")
            .to_string();
        let now = crate::studio::unix_seconds();
        // Reuse the canonical declaration for an already-declared remote Project.
        let declaration = self
            .store
            .workspaces()
            .declaration_for_path(&canonical_path, Some(alias));
        let existing = self
            .store
            .find_project_by_path(&canonical_path, Some(alias))
            .await?;
        let (id, created_at, name) = match (&declaration, existing) {
            (Some(declaration), row) => (
                declaration.id.clone(),
                row.map_or(now, |row| row.created_at),
                declaration.name.clone(),
            ),
            (None, Some(row)) => (row.id, row.created_at, row.name),
            (None, None) => (crate::studio::ids::new_id("project"), now, derived_name),
        };
        // The canonical declaration is written before the directory mutation so a newly
        // created SSH Project survives restart instead of disappearing with the cache.
        self.store
            .workspaces()
            .declare(&crate::config::WorkspaceDeclaration::new(
                id.clone(),
                name.clone(),
                canonical_path.clone(),
                Some(alias.to_string()),
            ))?;
        self.agent_facility
            .product_events
            .record_project_fact(crate::studio::product_event_bus::ProjectFact {
                id: id.clone(),
                name: name.clone(),
                path: canonical_path.clone(),
                ssh_alias: Some(alias.to_string()),
                created_at,
                updated_at: now,
                last_opened_at: Some(now),
                closed: false,
            })
            .await;
        let delta = ProjectDirectoryRecord {
            id: id.clone(),
            name: name.clone(),
            path: canonical_path.clone(),
            ssh_alias: Some(alias.to_string()),
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
            ssh_alias: Some(alias.to_string()),
            updated_at: now,
        })
    }
}

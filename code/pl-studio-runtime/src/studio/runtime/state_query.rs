use anyhow::Result;
use pl_protocol::ObservedStateMeta;

use crate::{
    StudioRecoveryStateSnapshot, StudioSettingsStateSnapshot, StudioSkillsStateSnapshot,
    StudioStateSnapshot, StudioThreadDirectoryPage,
};

use super::StudioRuntime;

const STATE_THREAD_PAGE_LIMIT: usize = 50;

impl StudioRuntime {
    /// Reads the complete canonical Studio state without causing lifecycle side effects.
    pub async fn read_state(&self) -> Result<StudioStateSnapshot> {
        let runtime = self.runtime_snapshot().await?;
        let project_directory = self
            .agent_facility
            .product_events
            .read_project_directory()
            .await?;
        let thread_directory = self
            .agent_facility
            .product_events
            .read_thread_directory_page(None, STATE_THREAD_PAGE_LIMIT)
            .await?;
        let task_directory = self
            .agent_facility
            .product_events
            .read_task_directory()
            .await?;
        let agent_directory = self
            .agent_facility
            .product_events
            .read_agent_directory()
            .await;
        let settings = self.read_settings()?;
        let recovery = StudioRecoveryStateSnapshot {
            meta: self.agent_facility.product_events.recovery_meta(),
            issues: self.recovery_issues(),
        };
        let mcp = self.read_mcp_state().await?;
        let lsp = self.read_lsp_state().await;
        let mut skills_by_project = Vec::with_capacity(project_directory.projects.len());
        for project in &project_directory.projects {
            skills_by_project.push(StudioSkillsStateSnapshot::from(
                self.skills.read(&project.id).await,
            ));
        }
        Ok(StudioStateSnapshot {
            runtime,
            project_directory,
            thread_directory,
            task_directory,
            agent_directory,
            settings: StudioSettingsStateSnapshot {
                meta: ObservedStateMeta::ready(settings.revision, settings.updated_at),
                settings: settings.settings,
            },
            recovery,
            mcp,
            lsp,
            skills_by_project,
            provider_usage: self.read_provider_usage_state().await,
            updater: self.read_update_state().await,
        })
    }

    /// Reads a keyset-paginated Thread directory page from the canonical in-memory index.
    pub async fn list_threads_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<StudioThreadDirectoryPage> {
        self.agent_facility
            .product_events
            .read_thread_directory_page(cursor, limit)
            .await
    }

    /// Reads one canonical Thread without activating its actor.
    pub async fn read_thread(&self, thread_id: &str) -> Result<pl_protocol::Thread> {
        self.read_protocol_thread(thread_id).await
    }

    /// Reads a Project Skills owner snapshot without scanning the filesystem.
    pub async fn read_skills_state(&self, project_id: &str) -> StudioSkillsStateSnapshot {
        self.skills.read(project_id).await.into()
    }

    /// Subscribes to canonical low-frequency product events.
    pub fn subscribe_product(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::StudioProductEventEnvelope> {
        self.agent_facility.product_events.subscribe()
    }
}

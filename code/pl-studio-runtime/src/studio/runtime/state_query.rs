use anyhow::Result;

use crate::{
    StudioSettingsStateSnapshot, StudioSkillsStateSnapshot, StudioStateSnapshot,
    StudioThreadDirectoryPage,
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
        let agent_directory = self
            .agent_facility
            .product_events
            .read_agent_directory()
            .await;
        let settings = self.read_settings()?;
        let recovery = crate::StudioRecoveryStateSnapshot {
            state: self.recovery.state(),
        };
        let mcp = self.read_mcp_state().await?;
        let lsp = self.read_lsp_state().await;
        let mut skills_by_project = Vec::new();
        if let Some(directory) = project_directory.state.value() {
            skills_by_project.reserve(directory.projects.len());
            for project in &directory.projects {
                skills_by_project.push(StudioSkillsStateSnapshot::from(
                    self.skills.read(&project.id).await,
                ));
            }
        }
        Ok(StudioStateSnapshot {
            runtime,
            project_directory,
            thread_directory,
            agent_directory,
            settings: StudioSettingsStateSnapshot {
                state: pl_protocol::ObservedResource::ready(
                    settings.revision,
                    settings.updated_at,
                    settings.settings,
                ),
            },
            recovery,
            mcp,
            lsp,
            skills_by_project,
            thread_mode_catalog: self.read_thread_mode_catalog(),
            provider_usage: self.read_provider_usage_state().await,
            model_performance: self.model_performance.snapshot().await,
            updater: self.read_update_state().await,
            persistence: self.agent_facility.product_events.persistence_state(),
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

    pub async fn query_threads(
        &self,
        query: &pl_protocol::studio::ThreadDirectoryQuery,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<StudioThreadDirectoryPage> {
        let mut page = self
            .agent_facility
            .product_events
            .query_threads(query, cursor, limit)
            .await?;
        if cursor.is_none() && !query.archived {
            let settings = self.config_runtime.read()?;
            let projects = self.agent_facility.product_events.project_snapshot().await;
            let mut pinned = Vec::new();
            for id in &settings.config.ui.pinned_thread_ids {
                let thread = match self.agent_facility.product_events.thread_snapshot(id) {
                    Some(thread) => Some(thread),
                    None => self
                        .store
                        .read_thread(id)
                        .await?
                        .map(pl_protocol::Thread::from),
                };
                if let Some(thread) = thread {
                    let project_matches = projects
                        .iter()
                        .find(|project| project.id == thread.project_id)
                        .is_some_and(|project| {
                            query.search.as_ref().is_some_and(|search| {
                                format!("{} {}", project.name, project.path)
                                    .to_lowercase()
                                    .contains(&search.trim().to_lowercase())
                            })
                        });
                    if query.matches(&thread, project_matches) {
                        pinned.push(thread);
                    }
                }
            }
            page.state = page.state.map(|mut data| {
                data.threads
                    .retain(|thread| !pinned.iter().any(|pin| pin.id == thread.id));
                pinned.extend(data.threads);
                data.threads = pinned;
                data
            });
        }
        Ok(page)
    }

    /// Reads one canonical Thread without activating its actor.
    pub async fn read_thread(&self, thread_id: &str) -> Result<pl_protocol::Thread> {
        self.read_protocol_thread(thread_id).await
    }

    /// Reads a Project Skills owner snapshot without scanning the filesystem.
    pub async fn read_skills_state(&self, project_id: &str) -> StudioSkillsStateSnapshot {
        self.skills.read(project_id).await.into()
    }

    /// Reads the current process-wide Thread Mode catalog.
    pub fn read_thread_mode_catalog(&self) -> pl_protocol::ThreadModeCatalogSnapshot {
        self.thread_modes.snapshot().catalog().clone()
    }

    /// Subscribes to canonical low-frequency product events.
    pub fn subscribe_product(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::StudioProductEventEnvelope> {
        self.agent_facility.product_events.subscribe()
    }
}

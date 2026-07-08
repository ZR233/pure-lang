use std::path::Path;

use anyhow::{Context, Result};

use crate::config::ConfigStore;
use crate::mcp::McpRuntimeRegistry;
use crate::resolve_workspace_root;
use crate::studio::active_turns::StudioActiveTurns;
use crate::studio::{
    InteractionRuntime, StudioEventRuntime, StudioRuntimeSnapshot, StudioRuntimeState,
    StudioRuntimeStatus, StudioStore,
};

use super::StudioRuntime;

impl StudioRuntime {
    pub async fn default_app() -> Result<Self> {
        let store = StudioStore::default_app().await?;
        let runtime = Self::with_runtime_state(
            store,
            ConfigStore::default_app()?,
            StudioRuntimeState::new(),
        );
        let _ = runtime.initialize_runtime().await?;
        Ok(runtime)
    }

    pub fn new(store: StudioStore, config_store: ConfigStore) -> Self {
        Self::with_runtime_state(store, config_store, StudioRuntimeState::ready())
    }

    pub(super) fn with_runtime_state(
        store: StudioStore,
        config_store: ConfigStore,
        runtime_state: StudioRuntimeState,
    ) -> Self {
        Self {
            interactions: InteractionRuntime::new(store.clone()),
            events: StudioEventRuntime::new(store.clone()),
            store,
            config_store,
            mcp_runtime: McpRuntimeRegistry::new(),
            mcp_health_watcher: Default::default(),
            lsp_runtime: pl_lsp::LspRuntimeRegistry::new(),
            runtime_state: runtime_state.clone(),
            active_turns: StudioActiveTurns::new(runtime_state),
        }
    }

    pub fn store(&self) -> &StudioStore {
        &self.store
    }

    pub fn interactions(&self) -> &InteractionRuntime {
        &self.interactions
    }

    pub fn events(&self) -> &StudioEventRuntime {
        &self.events
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.config_store
    }

    pub fn mcp_runtime(&self) -> &McpRuntimeRegistry {
        &self.mcp_runtime
    }

    pub fn lsp_runtime(&self) -> &pl_lsp::LspRuntimeRegistry {
        &self.lsp_runtime
    }

    pub fn runtime_snapshot(&self) -> StudioRuntimeSnapshot {
        self.runtime_state.snapshot()
    }

    pub async fn initialize_runtime(&self) -> Result<StudioRuntimeSnapshot> {
        if matches!(self.runtime_snapshot().status, StudioRuntimeStatus::Ready) {
            return Ok(self.runtime_snapshot());
        }
        let _ = self
            .runtime_state
            .transition(StudioRuntimeStatus::Initializing, None)?;
        let initialization = async {
            let turns = self
                .store
                .cancel_unfinished_turns("application restarted")
                .await?;
            self.cancel_recovered_transient_interactions(turns).await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        match initialization {
            Ok(()) => self
                .runtime_state
                .transition(StudioRuntimeStatus::Ready, None),
            Err(error) => {
                let message = format!("{error:#}");
                let _ = self
                    .runtime_state
                    .transition(StudioRuntimeStatus::Failed, Some(message));
                Err(error)
            }
        }
    }

    pub async fn start_runtime(&self) -> Result<StudioRuntimeSnapshot> {
        if !matches!(self.runtime_snapshot().status, StudioRuntimeStatus::Ready) {
            let _ = self.initialize_runtime().await?;
        }
        self.start_mcp_health_watcher().await;
        if let Err(error) = self.reconcile_mcp_runtime().await {
            let message = format!("{error:#}");
            let _ = self
                .runtime_state
                .transition(StudioRuntimeStatus::Failed, Some(message));
            return Err(error);
        }
        Ok(self.runtime_snapshot())
    }

    pub async fn shutdown_runtime(&self) -> Result<StudioRuntimeSnapshot> {
        let status = self.runtime_snapshot().status;
        if matches!(
            status,
            StudioRuntimeStatus::Stopped | StudioRuntimeStatus::Failed
        ) {
            return Ok(self.runtime_snapshot());
        }
        let _ = self
            .runtime_state
            .transition(StudioRuntimeStatus::ShuttingDown, None)?;
        self.active_turns.cancel_all_and_clear().await;
        self.stop_mcp_health_watcher().await;
        self.mcp_runtime.shutdown().await;
        self.lsp_runtime.shutdown().await;
        self.runtime_state
            .transition(StudioRuntimeStatus::Stopped, None)
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_runtime().await;
    }

    pub async fn reconcile_lsp_runtime_for_project(&self, project_id: &str) -> Result<()> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        self.lsp_runtime.reconcile_workspace(workspace_root).await;
        Ok(())
    }
}

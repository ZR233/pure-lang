use std::path::Path;

use anyhow::{Context, Result};

use crate::config::ConfigStore;
use crate::mcp::McpRuntimeRegistry;
use crate::resolve_workspace_root;
use crate::studio::active_turns::StudioActiveTurns;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{
    InteractionRuntime, StudioEventRuntime, StudioRuntimeSnapshot, StudioRuntimeState,
    StudioRuntimeStatus, StudioStore,
};

use super::StudioRuntime;
#[cfg(test)]
use super::continuation::ContinuationLauncher;
use super::continuation::ContinuationReason;
use super::continuation::ContinuationScheduler;

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
        let task_coordinator = std::sync::Arc::new(TaskCoordinator::new(store.clone()));
        let lifecycle_epoch =
            if matches!(runtime_state.snapshot().status, StudioRuntimeStatus::Ready) {
                1
            } else {
                0
            };
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
            task_coordinator,
            lifecycle_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            lifecycle_epoch: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(
                lifecycle_epoch,
            )),
            post_turn_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            continuation_scheduler: ContinuationScheduler::new(),
            continuation_launcher: None,
            #[cfg(test)]
            continuation_request_barrier: None,
            #[cfg(test)]
            continuation_pre_submit_barrier: None,
            #[cfg(test)]
            continuation_post_lifecycle_barrier: None,
            #[cfg(test)]
            continuation_launch_error_barrier: None,
            #[cfg(test)]
            prompt_finalization_barrier: None,
            #[cfg(test)]
            active_turn_removal_barrier: None,
            #[cfg(test)]
            shutdown_entry_barrier: None,
            #[cfg(test)]
            shutdown_after_cancel_barrier: None,
            #[cfg(test)]
            initialization_entry_barrier: None,
        }
    }

    #[cfg(test)]
    pub(super) fn with_runtime_state_and_continuation_launcher(
        store: StudioStore,
        config_store: ConfigStore,
        runtime_state: StudioRuntimeState,
        continuation_launcher: std::sync::Arc<dyn ContinuationLauncher>,
    ) -> Self {
        let mut runtime = Self::with_runtime_state(store, config_store, runtime_state);
        runtime.continuation_launcher = Some(continuation_launcher);
        runtime
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
        #[cfg(test)]
        if let Some(barrier) = &self.initialization_entry_barrier {
            barrier.wait().await;
        }
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
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
            self.task_coordinator.recover_active_tasks().await
        }
        .await;
        match initialization {
            Ok(recovered_runs) => {
                let ready = self
                    .runtime_state
                    .transition(StudioRuntimeStatus::Ready, None)?;
                let lifecycle_epoch = self.advance_lifecycle_epoch();
                self.continuation_scheduler.resume(lifecycle_epoch).await;
                for run in recovered_runs {
                    self.request_task_continuation(run.id, ContinuationReason::Recovery)
                        .await;
                }
                Ok(ready)
            }
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
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        #[cfg(test)]
        if let Some(barrier) = &self.shutdown_entry_barrier {
            barrier.pause_once().await;
        }
        let status = self.runtime_snapshot().status;
        if matches!(status, StudioRuntimeStatus::Stopped) {
            return Ok(self.runtime_snapshot());
        }
        let post_turn_guard = self.post_turn_lock.lock().await;
        let _ = self
            .runtime_state
            .transition(StudioRuntimeStatus::ShuttingDown, None)?;
        let _ = self.advance_lifecycle_epoch();
        self.continuation_scheduler.pause_and_clear().await;
        self.active_turns.cancel_all().await;
        #[cfg(test)]
        if let Some(barrier) = &self.shutdown_after_cancel_barrier {
            barrier.pause_once().await;
        }
        drop(post_turn_guard);
        self.active_turns.wait_until_empty().await;
        let _post_turn_guard = self.post_turn_lock.lock().await;
        self.task_coordinator.suspend();
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

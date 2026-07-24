use std::path::Path;

use anyhow::{Context, Result};

use crate::config::ConfigStore;
use crate::resolve_workspace_root;
use crate::studio::agent_host::{
    StudioAgentHost, StudioAgentResources, StudioAgentRuntime, StudioContinuationReason,
    StudioContinuationService, runtime_options,
};
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{
    InteractionRuntime, StudioProductEventRuntime, StudioRuntimeSnapshot, StudioRuntimeState,
    StudioRuntimeStatus, StudioStore,
};
use crate::{LocalMcpRuntimeHost, McpRuntime, McpRuntimeHandle};

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
        let task_coordinator = std::sync::Arc::new(TaskCoordinator::new(store.clone()));
        let interactions = InteractionRuntime::new(store.clone());
        let product_events = StudioProductEventRuntime::new(store.clone());
        let continuations = StudioContinuationService::new(store.clone(), task_coordinator.clone());
        Self {
            interactions,
            product_events,
            store,
            config_store,
            mcp_runtime: McpRuntime::new(LocalMcpRuntimeHost).handle(),
            mcp_health_watcher: Default::default(),
            lsp_runtime: pl_lsp::LspRuntimeRegistry::new(),
            runtime_state,
            agent_framework: Default::default(),
            agent_resources: StudioAgentResources::default(),
            continuations,
            task_coordinator,
            lifecycle_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(test)]
            initialization_entry_barrier: None,
        }
    }

    pub fn store(&self) -> &StudioStore {
        &self.store
    }

    pub fn interactions(&self) -> &InteractionRuntime {
        &self.interactions
    }

    pub fn product_events(&self) -> &StudioProductEventRuntime {
        &self.product_events
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.config_store
    }

    pub fn mcp_runtime(&self) -> &McpRuntimeHandle {
        &self.mcp_runtime
    }

    pub fn lsp_runtime(&self) -> &pl_lsp::LspRuntimeRegistry {
        &self.lsp_runtime
    }

    pub fn runtime_snapshot(&self) -> StudioRuntimeSnapshot {
        self.runtime_state.snapshot()
    }

    pub(super) async fn agent_framework(&self) -> Result<std::sync::Arc<StudioAgentRuntime>> {
        let mut framework = self.agent_framework.lock().await;
        if let Some(runtime) = framework.as_ref() {
            return Ok(runtime.clone());
        }
        self.agent_resources
            .restore_bindings(self.store.list_active_agent_sessions().await?)
            .await;
        let host = StudioAgentHost::new(
            self.store.clone(),
            self.config_store.clone(),
            self.mcp_runtime.clone(),
            self.lsp_runtime.clone(),
            self.interactions.clone(),
            self.runtime_state.clone(),
            self.continuations.clone(),
            self.task_coordinator.clone(),
            self.agent_resources.clone(),
            self.product_events.clone(),
        );
        let runtime = std::sync::Arc::new(
            StudioAgentRuntime::start(host, runtime_options())
                .await
                .map_err(|error| anyhow::anyhow!(error))?,
        );
        let handle = runtime.handle();
        runtime.host().attach_runtime(handle.clone()).await;
        handle
            .start_restored_inputs()
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        self.continuations.attach(handle).await;
        *framework = Some(runtime.clone());
        Ok(runtime)
    }

    /// 订阅 PL canonical session stream；首帧由 framework 决定为 snapshot 或 replay。
    pub async fn subscribe_session_events(
        &self,
        request: pl_protocol::SessionSubscriptionRequest,
    ) -> Result<pl_core::SessionEventSubscription> {
        let framework = self.agent_framework().await?;
        framework
            .handle()
            .subscribe_session(request)
            .map_err(|error| anyhow::anyhow!(error))
    }

    /// 读取包含尚未终态化 delta overlay 的 authoritative session snapshot。
    pub async fn session_event_snapshot(
        &self,
        session_id: &str,
    ) -> Result<pl_protocol::SessionViewSnapshot> {
        let framework = self.agent_framework().await?;
        let session_id = pl_core::SessionId::new(session_id.to_string())?;
        framework
            .handle()
            .session_snapshot(&session_id)
            .map_err(|error| anyhow::anyhow!(error))
    }

    async fn shutdown_agent_framework(&self) -> Result<()> {
        self.continuations.detach().await;
        let framework = self.agent_framework.lock().await.take();
        if let Some(framework) = framework {
            framework.host().detach_runtime().await;
            framework
                .shutdown()
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
        }
        Ok(())
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
            self.cancel_recovered_transient_interactions().await?;
            let recovered_runs = self.task_coordinator.recover_active_tasks().await?;
            Ok(recovered_runs)
        }
        .await;
        match initialization {
            Ok(recovered_runs) => {
                let ready = self
                    .runtime_state
                    .transition(StudioRuntimeStatus::Ready, None)?;
                for run in recovered_runs {
                    self.continuations
                        .request(run.id, StudioContinuationReason::Recovery);
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

    /// Stops all Studio runtime services.
    pub async fn shutdown_runtime(&self) -> Result<StudioRuntimeSnapshot> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.shutdown_runtime_locked().await
    }

    /// Stops the runtime only when no turn or durable task is active.
    ///
    /// Holding the lifecycle lock makes the final idle check atomic with the
    /// transition away from `Ready`; prompt submission uses the same lock.
    pub async fn shutdown_runtime_if_idle(&self) -> Result<Option<StudioRuntimeSnapshot>> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        if self.is_busy_for_update().await? {
            return Ok(None);
        }
        self.shutdown_runtime_locked().await.map(Some)
    }

    async fn shutdown_runtime_locked(&self) -> Result<StudioRuntimeSnapshot> {
        let status = self.runtime_snapshot().status;
        if matches!(status, StudioRuntimeStatus::Stopped) {
            return Ok(self.runtime_snapshot());
        }
        let _ = self
            .runtime_state
            .transition(StudioRuntimeStatus::ShuttingDown, None)?;
        self.shutdown_agent_framework().await?;
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

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::ConfigStore;
use crate::resolve_workspace_root;
use crate::studio::agent_host::{
    StudioAgentHost, StudioAgentRepository, StudioAgentResources, StudioAgentRuntime,
    runtime_options,
};
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{
    InteractionRuntime, StudioProductEventRuntime, StudioRecoveryIssue, StudioRecoveryIssueAction,
    StudioRecoveryIssueCategory, StudioRecoveryIssueScope, StudioRuntimeSnapshot,
    StudioRuntimeState, StudioRuntimeStatus, StudioStore,
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
        Self {
            interactions,
            product_events,
            store,
            config_store,
            mcp_runtime: McpRuntime::new(LocalMcpRuntimeHost).handle(),
            mcp_health_watcher: Default::default(),
            lsp_runtime: pl_lsp::LspRuntimeRegistry::new(),
            runtime_state,
            recovery: crate::studio::StudioRecoveryRegistry::new(),
            agent_framework: Default::default(),
            agent_resources: StudioAgentResources::default(),
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

    /// 返回当前所有恢复问题的快照。
    ///
    /// 恢复问题由独立的 [`StudioRecoveryRegistry`] 持有，不混入 runtime 快照，
    /// 避免与生命周期转换竞争同一把锁。
    pub fn recovery_issues(&self) -> Vec<StudioRecoveryIssue> {
        self.recovery.snapshot()
    }

    pub fn runtime_snapshot(&self) -> StudioRuntimeSnapshot {
        self.runtime_state.snapshot()
    }

    pub(in crate::studio) async fn agent_framework(
        &self,
    ) -> Result<std::sync::Arc<StudioAgentRuntime>> {
        let mut framework = self.agent_framework.lock().await;
        if let Some(runtime) = framework.as_ref() {
            return Ok(runtime.clone());
        }
        let host = StudioAgentHost::new(
            self.store.clone(),
            self.config_store.clone(),
            self.mcp_runtime.clone(),
            self.lsp_runtime.clone(),
            self.interactions.clone(),
            self.task_coordinator.clone(),
            self.agent_resources.clone(),
            self.product_events.clone(),
        );
        let repaired_roles = self.store.repair_root_thread_roles().await?;
        if repaired_roles > 0 {
            tracing::warn!(
                repaired_roles,
                "repaired root Thread roles before restoring Studio actors"
            );
        }
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
        *framework = Some(runtime.clone());
        Ok(runtime)
    }

    /// 订阅 PL canonical Thread stream；首帧固定为 authoritative snapshot。
    pub async fn subscribe_thread(
        &self,
        request: pl_protocol::ThreadSubscriptionRequest,
    ) -> Result<pl_core::ThreadEventSubscription> {
        let (handle, _) = self.ensure_thread_agent(&request.thread_id).await?;
        let thread_id = request.thread_id.clone();
        let mut subscription = handle
            .subscribe_thread(request)
            .map_err(|error| anyhow::anyhow!(error))?;
        subscription
            .replace_bootstrap_thread(self.read_protocol_thread(&thread_id).await?)
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(subscription)
    }

    /// 读取包含尚未终态化 delta overlay 的 authoritative Thread snapshot。
    pub async fn thread_snapshot(&self, thread_id: &str) -> Result<pl_protocol::ThreadSnapshot> {
        let (handle, _) = self.ensure_thread_agent(thread_id).await?;
        let core_thread_id = pl_core::ThreadId::new(thread_id.to_string())?;
        let mut snapshot = handle
            .thread_snapshot(&core_thread_id)
            .map_err(|error| anyhow::anyhow!(error))?;
        snapshot.thread = self.read_protocol_thread(thread_id).await?;
        Ok(snapshot)
    }

    async fn read_protocol_thread(&self, thread_id: &str) -> Result<pl_protocol::Thread> {
        Ok(self
            .store
            .read_thread(thread_id)
            .await?
            .context("selected Thread not found")?
            .into())
    }

    async fn shutdown_agent_framework(&self) -> Result<()> {
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
            self.recover_interactions_after_restart().await?;
            let mut report = self.task_coordinator.recover_active_tasks().await?;
            self.append_session_recovery_issues(&mut report.issues)
                .await?;
            self.append_unavailable_project_recovery_issues(&mut report.issues)
                .await?;
            Ok::<_, anyhow::Error>(report)
        }
        .await;
        match initialization {
            Ok(report) => {
                self.recovery.replace(report.issues);
                self.runtime_state
                    .transition(StudioRuntimeStatus::Ready, None)
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

    pub(super) async fn append_unavailable_project_recovery_issues(
        &self,
        recovery_issues: &mut Vec<StudioRecoveryIssue>,
    ) -> Result<()> {
        for project in self.store.list_projects().await? {
            let Err(error) = resolve_workspace_root(Path::new(&project.path)) else {
                continue;
            };
            if recovery_issues.iter().any(|issue| {
                issue.scope == StudioRecoveryIssueScope::Project
                    && issue.project_id.as_deref() == Some(project.id.as_str())
            }) {
                continue;
            }
            recovery_issues.push(StudioRecoveryIssue {
                id: format!("recovery-issue-project-path-{}", project.id),
                scope: StudioRecoveryIssueScope::Project,
                category: StudioRecoveryIssueCategory::Repository,
                action: StudioRecoveryIssueAction::RemoveProject,
                project_id: Some(project.id),
                thread_id: None,
                task_run_id: None,
                message: format!("Project workspace is unavailable: {error}"),
            });
        }
        Ok(())
    }

    async fn append_session_recovery_issues(
        &self,
        recovery_issues: &mut Vec<StudioRecoveryIssue>,
    ) -> Result<()> {
        let failures = StudioAgentRepository::new(self.store.clone())
            .audit_registered_sessions()
            .await?;
        let mut failures_by_root = BTreeMap::<(String, String), Vec<_>>::new();
        for failure in failures {
            failures_by_root
                .entry((failure.project_id.clone(), failure.root_thread_id.clone()))
                .or_default()
                .push(failure);
        }
        for ((project_id, root_thread_id), failures) in failures_by_root {
            let task_run_id = self
                .store
                .find_active_task_run_for_root_thread(&root_thread_id)
                .await?
                .map(|run| run.id);
            let affected = failures
                .iter()
                .map(|failure| failure.agent_thread_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let detail = failures
                .first()
                .map(|failure| failure.detail.as_str())
                .unwrap_or("invalid durable session snapshot");
            recovery_issues.push(StudioRecoveryIssue {
                id: format!("session-context-{root_thread_id}"),
                scope: StudioRecoveryIssueScope::Thread,
                category: StudioRecoveryIssueCategory::AgentState,
                action: StudioRecoveryIssueAction::CleanupThread,
                project_id: Some(project_id),
                thread_id: Some(root_thread_id),
                task_run_id,
                message: format!(
                    "Durable Agent session context is invalid for {affected}: {detail}"
                ),
            });
        }
        Ok(())
    }
}

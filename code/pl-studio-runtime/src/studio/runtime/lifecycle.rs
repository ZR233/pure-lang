use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::{ConfigRuntime, ConfigRuntimeSnapshot, ConfigStore};
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
use crate::{McpConnector, McpRuntime, McpRuntimeHandle};

use super::StudioRuntime;

impl StudioRuntime {
    pub async fn default_app() -> Result<Self> {
        let store = StudioStore::default_app().await?;
        Self::with_runtime_state(
            store,
            ConfigStore::default_app()?,
            StudioRuntimeState::new(),
        )
    }

    pub fn new(store: StudioStore, config_store: ConfigStore) -> Result<Self> {
        Self::with_runtime_state(store, config_store, StudioRuntimeState::ready())
    }

    pub(super) fn with_runtime_state(
        store: StudioStore,
        config_store: ConfigStore,
        runtime_state: StudioRuntimeState,
    ) -> Result<Self> {
        let config_runtime = ConfigRuntime::initialize(config_store)?;
        let task_coordinator = std::sync::Arc::new(TaskCoordinator::new(store.clone()));
        let interactions = InteractionRuntime::new(store.clone());
        let product_events = StudioProductEventRuntime::new(store.clone());
        let provider_usage =
            super::ProviderUsageRuntime::new(store.clone(), product_events.clone());
        let updater = super::StudioUpdateRuntime::new(store.clone(), product_events.clone())?;
        let mcp_shared_tools = std::sync::Arc::new(pl_core::ToolRegistry::new());
        let mcp_state = super::mcp_health::McpStateRuntime::new();
        let lsp_state = super::lsp_state::LspStateRuntime::new(product_events.clone());
        Ok(Self {
            store,
            residency: super::residency::ThreadResidency::new(),
            shutdown_progress: super::StudioShutdownProgressRuntime::new(),
            config_runtime,
            external_runtimes: super::StudioExternalRuntimes {
                mcp: McpRuntime::new(McpConnector::default(), Some(mcp_shared_tools.clone()))
                    .handle(),
                mcp_shared_tools,
                mcp_state,
                mcp_startup_reconcile: Default::default(),
                mcp_health_watcher: Default::default(),
                lsp: pl_lsp::LspRuntimeRegistry::new(),
                lsp_state,
                lsp_state_watcher: Default::default(),
            },
            agent_facility: super::StudioAgentFacility {
                framework: Default::default(),
                resources: StudioAgentResources::default(),
                interactions,
                persistence: Default::default(),
                product_events: product_events.clone(),
            },
            runtime_state,
            recovery: crate::studio::StudioRecoveryRegistry::new(),
            skills: super::SkillCatalogRuntime::new(product_events.clone()),
            provider_usage,
            updater,
            activation: Default::default(),
            task_coordinator,
            lifecycle_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(test)]
            initialization_entry_barrier: None,
        })
    }

    pub fn store(&self) -> &StudioStore {
        &self.store
    }

    pub fn interactions(&self) -> &InteractionRuntime {
        &self.agent_facility.interactions
    }

    pub fn product_events(&self) -> &StudioProductEventRuntime {
        &self.agent_facility.product_events
    }

    pub fn config_runtime(&self) -> &ConfigRuntime {
        &self.config_runtime
    }

    pub fn settings_state(&self) -> Result<ConfigRuntimeSnapshot> {
        Ok(self.config_runtime.read()?)
    }

    pub fn publish_settings_state(&self, settings: ConfigRuntimeSnapshot) {
        self.agent_facility.product_events.emit_settings_state(
            crate::StudioSettingsStateSnapshot {
                meta: pl_protocol::ObservedStateMeta::ready(settings.revision, settings.updated_at),
                settings,
            },
        );
    }

    pub fn mcp_runtime(&self) -> &McpRuntimeHandle {
        &self.external_runtimes.mcp
    }

    pub fn lsp_runtime(&self) -> &pl_lsp::LspRuntimeRegistry {
        &self.external_runtimes.lsp
    }

    pub(super) async fn start_lsp_state_watcher(&self) {
        let mut watcher = self.external_runtimes.lsp_state_watcher.lock().await;
        if watcher.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }
        let runtime = self.clone();
        let mut updates = self.external_runtimes.lsp.subscribe();
        *watcher = Some(tokio::spawn(async move {
            while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
                updates.recv().await
            {
                let health = super::lsp_state::health(&runtime.external_runtimes.lsp).await;
                runtime.external_runtimes.lsp_state.refresh(health).await;
            }
        }));
    }

    pub(super) async fn stop_lsp_state_watcher(&self) {
        if let Some(handle) = self.external_runtimes.lsp_state_watcher.lock().await.take() {
            handle.abort();
        }
    }

    /// 返回当前所有恢复问题的快照。
    ///
    /// 恢复问题由独立的 [`StudioRecoveryRegistry`] 持有，不混入 runtime 快照，
    /// 避免与生命周期转换竞争同一把锁。
    pub fn recovery_issues(&self) -> Vec<StudioRecoveryIssue> {
        self.recovery.snapshot()
    }

    pub async fn runtime_snapshot(&self) -> Result<StudioRuntimeSnapshot> {
        let mut snapshot = self.runtime_state.snapshot();
        snapshot.active_turns = self.derive_active_turns().await?;
        Ok(snapshot)
    }

    pub(in crate::studio) async fn agent_framework(
        &self,
    ) -> Result<std::sync::Arc<StudioAgentRuntime>> {
        let mut framework = self.agent_facility.framework.lock().await;
        if let Some(runtime) = framework.as_ref() {
            return Ok(runtime.clone());
        }
        let host = StudioAgentHost::new(
            self.store.clone(),
            self.config_runtime.clone(),
            self.external_runtimes.mcp.clone(),
            self.external_runtimes.mcp_shared_tools.clone(),
            self.external_runtimes.lsp.clone(),
            self.agent_facility.interactions.clone(),
            self.task_coordinator.clone(),
            self.agent_facility.resources.clone(),
            self.agent_facility.product_events.clone(),
            self.skills.clone(),
        );
        // 记录 host 内部 repository 句柄，让 framework 被 take 后关机仍能排空 write-behind 队列。
        *self.agent_facility.persistence.lock().await = Some(host.persistence());
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
        *framework = Some(runtime.clone());
        drop(framework);
        // Pending wake/continuation 目标必须先驻留，attach 时 materialize 才不会
        // 跳过（wake）或破坏性失败（executor continuation）。
        let mut activation_targets = Vec::new();
        for wake in self.store.list_pending_task_planner_wakes().await? {
            activation_targets.push(wake.root_thread_id);
        }
        for continuation in self.store.list_pending_executor_continuations().await? {
            activation_targets.push(continuation.agent_id);
        }
        for target in activation_targets {
            // Box::pin 引入间接层，避免与 ensure_thread_agent 的 async 递归。
            if let Err(error) = Box::pin(self.ensure_thread_agent(&target)).await {
                tracing::warn!(
                    thread_id = %target,
                    error_bytes = error.to_string().len(),
                    "failed to activate a durable wake target at startup"
                );
            }
        }
        let handle = runtime.handle();
        runtime.host().attach_runtime(handle.clone()).await;
        handle
            .start_restored_inputs()
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(runtime)
    }

    /// 订阅 pin：guard 存活期间该线程不参与 LRU 淘汰，Drop 时自动解除。
    ///
    /// bridge 订阅 producer task 持有该 guard——订阅取消/流关闭即解除 pin。
    pub fn pin_thread(&self, thread_id: &str) -> ThreadResidencyPin {
        self.residency.pin(thread_id);
        ThreadResidencyPin {
            runtime: self.clone(),
            thread_id: thread_id.to_string(),
        }
    }

    /// 订阅 PL canonical Thread stream；首帧固定为 authoritative snapshot。
    ///
    /// 订阅是显式激活命令：未驻留的 Thread 在这里按需恢复。
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
        let repository = StudioAgentRepository::new(self.store.clone());
        let mut snapshot = repository
            .read_thread_snapshot(thread_id)
            .await?
            .context("selected Thread not found")?;
        if let Some((handle, _)) = self.try_get_thread_handle(thread_id).await? {
            let core_thread_id = pl_core::ThreadId::new(thread_id.to_string())?;
            snapshot = handle
                .thread_snapshot(&core_thread_id)
                .map_err(|error| anyhow::anyhow!(error))?;
            snapshot.thread = self.read_protocol_thread(thread_id).await?;
        }
        Ok(snapshot)
    }

    pub fn skill_catalog_runtime(&self) -> &super::SkillCatalogRuntime {
        &self.skills
    }

    pub async fn read_provider_usage_state(&self) -> super::ProviderUsageStateSnapshot {
        self.provider_usage.read().await
    }

    pub async fn check_provider_usage(&self) -> Result<super::ProviderUsageStateSnapshot> {
        let config = self.config_runtime.read()?.config;
        self.provider_usage.check(&config).await
    }

    pub async fn apply_provider_config(
        &self,
        config: &crate::StudioConfig,
    ) -> Result<super::ProviderUsageStateSnapshot> {
        self.provider_usage.apply_config(config).await
    }

    pub async fn read_update_state(&self) -> super::StudioUpdateStateSnapshot {
        self.updater.read().await
    }

    pub async fn check_studio_update(&self) -> Result<super::StudioUpdateStateSnapshot> {
        self.updater.check().await
    }

    pub async fn read_lsp_state(&self) -> crate::StudioLspStateSnapshot {
        self.external_runtimes.lsp_state.read().await
    }

    pub fn update_runtime(&self) -> &super::StudioUpdateRuntime {
        &self.updater
    }

    /// 返回已存在 actor 的 handle；查询路径不得初始化 framework 或注册 actor。
    pub async fn try_get_thread_handle(
        &self,
        thread_id: &str,
    ) -> Result<Option<(pl_core::AgentRuntimeHandle, pl_core::AgentId)>> {
        let Some(framework) = self.agent_facility.framework.lock().await.clone() else {
            return Ok(None);
        };
        let thread = self
            .store
            .read_thread(thread_id)
            .await?
            .context("selected Thread not found")?;
        let agent_id = pl_core::AgentId::new(thread.agent_path)?;
        let handle = framework.handle();
        let is_registered = handle
            .directory_snapshot()
            .agents
            .iter()
            .any(|agent| agent.identity.id == agent_id);
        Ok(is_registered.then_some((handle, agent_id)))
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
        let framework = self.agent_facility.framework.lock().await.take();
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
        if matches!(
            self.runtime_snapshot().await?.status,
            StudioRuntimeStatus::Ready
        ) {
            return self.runtime_snapshot().await;
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
                self.agent_facility
                    .product_events
                    .emit_recovery_state(self.recovery.snapshot());
                let _ = self
                    .runtime_state
                    .transition(StudioRuntimeStatus::Ready, None)?;
                self.runtime_snapshot().await
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
        if !matches!(
            self.runtime_snapshot().await?.status,
            StudioRuntimeStatus::Ready
        ) {
            let _ = self.initialize_runtime().await?;
        }
        let settings = self.config_runtime.read()?;
        self.provider_usage.load_cache().await?;
        self.updater.load_cache().await?;
        self.agent_facility
            .product_events
            .initialize_directories()
            .await?;
        // 惰性驻留：这里只启动 framework（restore_runtime 恢复钉住集合），
        // 其余 Thread 在订阅、提交输入或修复时按需恢复。
        let _ = self.agent_framework().await?;
        self.start_mcp_health_watcher().await;
        self.start_lsp_state_watcher().await;
        self.start_mcp_reconcile_background().await?;
        if settings.config.skills.system.enabled {
            let _ = pl_core::skill::install_system_skills(&settings.config.skills)?;
        }
        self.publish_settings_state(settings);
        self.agent_facility
            .product_events
            .emit_recovery_state(self.recovery.snapshot());
        self.runtime_snapshot().await
    }

    /// 淘汰超出 LRU 容量的空闲驻留 actor；淘汰前先排空 pending commits。
    pub(super) async fn enforce_residency_limit(&self) {
        let candidates = self.residency.over_capacity().await;
        if candidates.is_empty() {
            return;
        }
        let Some(framework) = self.agent_facility.framework.lock().await.clone() else {
            return;
        };
        let handle = framework.handle();
        for thread_id in candidates {
            // 有活跃订阅的线程不淘汰（design/17：订阅是显式观察者）。
            if self.residency.is_pinned(&thread_id) {
                continue;
            }
            let agent_id = match self.store.read_thread(&thread_id).await {
                Ok(Some(record)) => match pl_core::AgentId::new(record.agent_path) {
                    Ok(agent_id) => agent_id,
                    Err(error) => {
                        tracing::warn!(
                            thread_id = %thread_id,
                            error_bytes = error.to_string().len(),
                            "resident thread has an invalid agent path"
                        );
                        self.residency.remove(&thread_id).await;
                        continue;
                    }
                },
                Ok(None) => {
                    // Thread 已删除/归档；从驻留队列清除。
                    self.residency.remove(&thread_id).await;
                    continue;
                }
                Err(error) => {
                    tracing::warn!(
                        thread_id = %thread_id,
                        error_bytes = error.to_string().len(),
                        "failed to read resident thread for eviction"
                    );
                    continue;
                }
            };
            // busy 的候选移回队尾，等下一轮再试。
            match handle.snapshot(agent_id.clone()).await {
                Ok(snapshot)
                    if snapshot.active_turn_id.is_none() && snapshot.pending_inputs == 0 =>
                {
                    if let Some(repository) = self.agent_facility.persistence.lock().await.clone()
                        && let Err(error) = repository.writer().flush().await
                    {
                        tracing::warn!(
                            thread_id = %thread_id,
                            error_bytes = error.to_string().len(),
                            "failed to flush pending commits before eviction; retrying later"
                        );
                        self.residency.touch(&thread_id).await;
                        continue;
                    }
                    match handle.evict_agent(agent_id).await {
                        Ok(()) => {
                            self.residency.remove(&thread_id).await;
                            tracing::debug!(
                                thread_id = %thread_id,
                                "evicted idle resident thread actor"
                            );
                        }
                        Err(error) => {
                            tracing::warn!(
                                thread_id = %thread_id,
                                error_bytes = error.to_string().len(),
                                "failed to evict idle resident thread actor"
                            );
                            self.residency.touch(&thread_id).await;
                        }
                    }
                }
                _ => {
                    self.residency.touch(&thread_id).await;
                }
            }
        }
    }

    /// 显式修复缺失的 Thread actor，并恢复其 durable mailbox/wake。
    pub async fn repair_thread_runtime(&self, thread_id: &str) -> Result<()> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let _ = self.ensure_thread_agent(thread_id).await?;
        Ok(())
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
        let status = self.runtime_snapshot().await?.status;
        if matches!(status, StudioRuntimeStatus::Stopped) {
            return self.runtime_snapshot().await;
        }
        let _ = self
            .runtime_state
            .transition(StudioRuntimeStatus::ShuttingDown, None)?;
        // 阶段 1：订阅方自行消费进度流后由 bridge 取消订阅。
        self.shutdown_progress
            .emit(crate::StudioShutdownPhase::StoppingSubscriptions, 0);
        // 阶段 2：中断所有活动 Turn 并等待 actor 收束。
        self.shutdown_progress
            .emit(crate::StudioShutdownPhase::CancellingTurns, 0);
        self.shutdown_agent_framework().await?;
        // 阶段 3：等待 write-behind 全部 pending commit 落库；完成事件必须 pending=0。
        let pending_before = self.pending_persistence_commits().await;
        self.shutdown_progress.emit(
            crate::StudioShutdownPhase::FlushingPersistence,
            pending_before as u64,
        );
        if let Err(error) = self.flush_persistence().await {
            tracing::error!(
                error_bytes = error.to_string().len(),
                "failed to drain write-behind persistence during shutdown"
            );
        }
        self.shutdown_progress
            .emit(crate::StudioShutdownPhase::FlushingPersistence, 0);
        // 阶段 4：挂起 Task 协调。
        self.shutdown_progress
            .emit(crate::StudioShutdownPhase::SuspendingTasks, 0);
        self.task_coordinator.suspend();
        // 阶段 5：关闭 MCP。
        self.shutdown_progress
            .emit(crate::StudioShutdownPhase::StoppingMcp, 0);
        self.stop_mcp_startup_reconcile().await;
        self.stop_mcp_health_watcher().await;
        self.stop_lsp_state_watcher().await;
        self.external_runtimes.mcp.shutdown().await;
        self.publish_mcp_stopped().await;
        // 阶段 6：关闭 LSP。
        self.shutdown_progress
            .emit(crate::StudioShutdownPhase::StoppingLsp, 0);
        self.external_runtimes.lsp.shutdown().await;
        self.external_runtimes.lsp_state.stopped().await;
        let _ = self
            .runtime_state
            .transition(StudioRuntimeStatus::Stopped, None)?;
        // 阶段 7：终态。
        self.shutdown_progress
            .emit(crate::StudioShutdownPhase::Stopped, 0);
        self.runtime_snapshot().await
    }

    /// 订阅关机阶段进度；通道随 runtime 共享，并发 shutdown 共享同一次序列。
    pub async fn subscribe_shutdown_progress(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::StudioShutdownProgress> {
        self.shutdown_progress.subscribe()
    }

    /// 排空 agent framework 的 write-behind 队列并停止 writer。
    async fn flush_persistence(&self) -> Result<()> {
        let repository = self.agent_facility.persistence.lock().await.take();
        if let Some(repository) = repository {
            repository
                .writer()
                .shutdown()
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        Ok(())
    }

    /// 当前尚未落库的 pending commit 数量（关机进度用）。
    pub async fn pending_persistence_commits(&self) -> usize {
        let repository = self.agent_facility.persistence.lock().await.clone();
        repository.map_or(0, |repository| repository.writer().pending_commit_count())
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_runtime().await;
    }

    pub async fn activate_project(&self, project_id: &str) -> Result<()> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        let settings = self.config_runtime.read()?;
        // 自定义 LSP server 与 catalog 冲突在配置加载时已 fail-loud；此处再合并一次，
        // 保证 registry catalog 与当前配置一致，并让 fingerprint 覆盖检测结果。
        self.external_runtimes
            .lsp
            .apply_user_servers(&settings.config.lsp.servers)
            .await
            .map_err(|error| anyhow::anyhow!("invalid [lsp.servers] configuration: {error}"))?;
        let fingerprint = format!(
            "{}:{}:{}",
            workspace_root.display(),
            self.external_runtimes
                .lsp
                .membership_fingerprint(&workspace_root)
                .await,
            super::skill_catalog::skills_fingerprint(&settings.config.skills)?,
        );
        let _activation_command = self.activation.command_lock.lock().await;
        if self
            .activation
            .applied
            .read()
            .await
            .as_ref()
            .is_some_and(|applied| {
                applied.project_id == project_id && applied.fingerprint == fingerprint
            })
        {
            return Ok(());
        }
        let _command = self.external_runtimes.lsp_state.command().await;
        self.external_runtimes
            .lsp_state
            .begin(pl_protocol::StateOperation::Activate)
            .await;
        self.external_runtimes
            .lsp
            .reconcile_workspace_membership(&workspace_root)
            .await;
        self.external_runtimes
            .lsp
            .probe_lsp_server(&workspace_root)
            .await;
        let health = super::lsp_state::health(&self.external_runtimes.lsp).await;
        self.external_runtimes.lsp_state.ready(health, true).await;
        let _ = self
            .skills
            .discover(project_id, &workspace_root, &settings.config.skills)
            .await?;
        *self.activation.applied.write().await = Some(super::ProjectActivation {
            project_id: project_id.to_string(),
            fingerprint,
        });
        Ok(())
    }

    pub async fn discover_skills(&self, project_id: &str) -> Result<super::SkillsStateSnapshot> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        let settings = self.config_runtime.read()?;
        self.skills
            .discover(project_id, &workspace_root, &settings.config.skills)
            .await
    }

    pub async fn probe_lsp_server(&self, project_id: &str) -> Result<()> {
        let workspace_root = self.project_workspace_root(project_id).await?;
        let _command = self.external_runtimes.lsp_state.command().await;
        self.external_runtimes
            .lsp_state
            .begin(pl_protocol::StateOperation::Probe)
            .await;
        self.external_runtimes
            .lsp
            .probe_lsp_server(workspace_root)
            .await;
        let health = super::lsp_state::health(&self.external_runtimes.lsp).await;
        self.external_runtimes.lsp_state.ready(health, true).await;
        Ok(())
    }

    pub async fn repair_lsp_server(&self, project_id: &str, server_id: &str) -> Result<()> {
        let workspace_root = self.project_workspace_root(project_id).await?;
        let _command = self.external_runtimes.lsp_state.command().await;
        self.external_runtimes
            .lsp_state
            .begin(pl_protocol::StateOperation::Repair)
            .await;
        let result = self
            .external_runtimes
            .lsp
            .repair_lsp_server(workspace_root, server_id)
            .await
            .map_err(anyhow::Error::from);
        match result {
            Ok(()) => {
                let health = super::lsp_state::health(&self.external_runtimes.lsp).await;
                self.external_runtimes.lsp_state.ready(health, true).await;
                Ok(())
            }
            Err(error) => {
                self.external_runtimes
                    .lsp_state
                    .failed(pl_protocol::StateOperation::Repair, &error, true)
                    .await;
                Err(error)
            }
        }
    }

    pub async fn reset_lsp(&self, scope: pl_lsp::LspScope) -> Result<()> {
        let _command = self.external_runtimes.lsp_state.command().await;
        self.external_runtimes
            .lsp_state
            .begin(pl_protocol::StateOperation::Reset)
            .await;
        let result = self
            .external_runtimes
            .lsp
            .reset_lsp(scope)
            .await
            .map_err(anyhow::Error::from);
        match result {
            Ok(()) => {
                let health = super::lsp_state::health(&self.external_runtimes.lsp).await;
                self.external_runtimes.lsp_state.ready(health, false).await;
                Ok(())
            }
            Err(error) => {
                self.external_runtimes
                    .lsp_state
                    .failed(pl_protocol::StateOperation::Reset, &error, false)
                    .await;
                Err(error)
            }
        }
    }

    pub async fn project_workspace_root(&self, project_id: &str) -> Result<std::path::PathBuf> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        Ok(resolve_workspace_root(Path::new(&project.path))?)
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

/// 订阅驻留 pin guard：Drop 时解除 pin。
pub struct ThreadResidencyPin {
    runtime: StudioRuntime,
    thread_id: String,
}

impl Drop for ThreadResidencyPin {
    fn drop(&mut self) {
        self.runtime.residency.unpin(&self.thread_id);
    }
}

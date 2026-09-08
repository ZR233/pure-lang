use anyhow::{Context, Result};

use crate::studio::agent_host::{
    StudioAgentHost, StudioAgentRepository, StudioAgentRuntime, runtime_options,
};

use super::super::StudioRuntime;
use super::super::background_task::{self, BackgroundTask};
use super::super::lsp_state::health;

/// Owns initialization progress as well as the runtime needed for failed-start cleanup.
pub(in crate::studio::runtime) struct FrameworkOwner {
    runtime: std::sync::Arc<StudioAgentRuntime>,
    ready: tokio::sync::OnceCell<()>,
    closing: tokio_util::sync::CancellationToken,
}

impl FrameworkOwner {
    pub(in crate::studio::runtime) fn ready_runtime(
        &self,
    ) -> Option<std::sync::Arc<StudioAgentRuntime>> {
        (self.ready.get().is_some() && !self.closing.is_cancelled()).then(|| self.runtime.clone())
    }

    async fn initialize(&self) -> Result<std::sync::Arc<StudioAgentRuntime>> {
        anyhow::ensure!(!self.closing.is_cancelled(), "agent framework is closing");
        tokio::select! {
            result = self.ready.get_or_try_init(|| async {
                self.runtime.handle().start_restored_inputs().await.map_err(|error| anyhow::anyhow!(error))
            }) => { result?; }
            _ = self.closing.cancelled() => anyhow::bail!("agent framework closed during initialization"),
        }
        self.ready_runtime()
            .context("agent framework closed during initialization")
    }
}

impl StudioRuntime {
    pub(in crate::studio) async fn agent_framework(
        &self,
    ) -> Result<std::sync::Arc<StudioAgentRuntime>> {
        let mut framework = self.agent_facility.framework.lock().await;
        let state = self.runtime_state.snapshot().state.kind();
        anyhow::ensure!(
            matches!(
                state,
                crate::studio::StudioRuntimeStateKind::Initializing
                    | crate::studio::StudioRuntimeStateKind::Ready
            ),
            "cannot create agent framework while Studio runtime is {state:?}"
        );
        if framework.is_none() {
            let persistence = self
                .agent_facility
                .persistence
                .lock()
                .await
                .clone()
                .context("Studio persistence writer is unavailable")?;
            let host = StudioAgentHost::new(
                persistence,
                self.agent_facility.worktrees.clone(),
                self.store.clone(),
                self.config_runtime.clone(),
                self.external_runtimes.mcp.clone(),
                self.agent_facility.tool_manager.clone(),
                self.external_runtimes.lsp.clone(),
                self.agent_facility.interactions.clone(),
                self.agent_facility.resources.clone(),
                self.agent_facility.product_events.clone(),
                self.skills.clone(),
                self.thread_modes.clone(),
                self.ssh_manager.clone(),
            );
            let runtime = std::sync::Arc::new(
                StudioAgentRuntime::start(host, runtime_options())
                    .await
                    .map_err(|error| anyhow::anyhow!(error))?,
            );
            *framework = Some(std::sync::Arc::new(FrameworkOwner {
                runtime,
                ready: tokio::sync::OnceCell::new(),
                closing: tokio_util::sync::CancellationToken::new(),
            }));
        }
        let owner = framework
            .as_ref()
            .context("agent framework owner missing")?
            .clone();
        drop(framework);
        owner.initialize().await
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
        request: pl_core::ThreadSubscriptionRequest,
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
    pub async fn thread_snapshot(&self, thread_id: &str) -> Result<pl_core::ThreadSnapshot> {
        let (handle, agent_id) = self.ensure_thread_agent(thread_id).await?;
        let mut snapshot = handle
            .thread_snapshot(&agent_id)
            .map_err(|error| anyhow::anyhow!(error))?;
        snapshot.thread = self.read_protocol_thread(thread_id).await?;
        Ok(snapshot)
    }

    /// 查询路径使用的只读 repository 句柄（共享进程级 writer 的实例）。
    pub(in crate::studio) async fn persistence_repository(&self) -> Option<StudioAgentRepository> {
        self.agent_facility.persistence.lock().await.clone()
    }

    /// 返回已存在 actor 的 handle；查询路径不得初始化 framework 或注册 actor。
    ///
    /// Studio Thread 的 runtime 身份恒等于其 Thread id（design/17 注册约定），
    /// 因此驻留判定只看 runtime 目录；热集合未命中不阻断冷数据查询。
    pub(crate) async fn try_get_thread_handle(
        &self,
        thread_id: &str,
    ) -> Result<Option<(pl_core::AgentRuntimeHandle, pl_core::ThreadId)>> {
        let Some(framework) = self
            .agent_facility
            .framework
            .lock()
            .await
            .as_ref()
            .and_then(|owner| owner.ready_runtime())
        else {
            return Ok(None);
        };
        let agent_id = pl_core::ThreadId::new(thread_id.to_string())?;
        let handle = framework.handle();
        let is_registered = handle
            .directory_snapshot()
            .agents
            .iter()
            .any(|agent| agent.identity.id == agent_id);
        Ok(is_registered.then_some((handle, agent_id)))
    }

    pub(in crate::studio::runtime) async fn read_protocol_thread(
        &self,
        thread_id: &str,
    ) -> Result<pl_core::Thread> {
        if let Some(thread) = self
            .agent_facility
            .product_events
            .thread_snapshot(thread_id)
        {
            return Ok(thread);
        }
        // 冷数据回源：未驻留 Thread 的目录元数据从 SQLite 读取。
        let Some(record) = self.store.read_thread(thread_id).await? else {
            return Err(anyhow::anyhow!("selected Thread not found"));
        };
        Ok(pl_core::Thread::from(record))
    }

    pub(super) async fn start_lsp_state_watcher(&self) {
        let mut watcher = self.external_runtimes.lsp_state_watcher.lock().await;
        if watcher.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }
        let runtime = self.clone();
        let mut updates = self.external_runtimes.lsp.subscribe();
        *watcher = Some(BackgroundTask::new(tokio::spawn(async move {
            while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
                updates.recv().await
            {
                let health = health(&runtime.external_runtimes.lsp).await;
                if let Err(error) = runtime.external_runtimes.lsp_state.refresh(health).await {
                    tracing::warn!(%error, "failed to refresh LSP observed state");
                }
            }
        })));
    }

    pub(super) async fn stop_lsp_state_watcher(&self) -> Result<()> {
        background_task::stop(&self.external_runtimes.lsp_state_watcher)
            .await
            .context("failed to join LSP state watcher")
    }

    /// 淘汰超出 LRU 容量且已保存的空闲驻留 actor；未保存时保留内存。
    pub(in crate::studio::runtime) async fn enforce_residency_limit(&self) {
        let candidates = self.residency.over_capacity().await;
        if candidates.is_empty() {
            return;
        }
        let Some(framework) = self
            .agent_facility
            .framework
            .lock()
            .await
            .as_ref()
            .and_then(|owner| owner.ready_runtime())
        else {
            return;
        };
        let handle = framework.handle();
        for thread_id in candidates {
            // 候选计算已排除活跃订阅；这里再次检查订阅 pin，覆盖
            // 候选快照生成后到实际逐出前新建订阅的竞争。
            if self.residency.is_pinned(&thread_id)
                || self.agent_facility.worktrees.get(&thread_id).is_some_and(|lease| lease.state != crate::studio::agent_host::worktree_lease::WorktreeLeaseState::Cleaned) {
                continue;
            }
            let agent_id = match pl_core::ThreadId::new(thread_id.clone()) {
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
            };
            // busy 的候选移回队尾，等下一轮再试。
            match handle.snapshot(agent_id.clone()).await {
                Ok(snapshot)
                    if snapshot.active_turn_id().is_none() && snapshot.pending_inputs == 0 =>
                {
                    match handle.evict_agent(agent_id).await {
                        Ok(()) => {
                            self.residency.remove(&thread_id).await;
                            self.agent_facility
                                .resources
                                .evict_thread_attachments(&thread_id)
                                .await;
                            // 耐久化完成后热集合条目退回冷数据，由分页查询回源。
                            self.agent_facility
                                .product_events
                                .evict_thread_entry(&thread_id);
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

    pub(super) async fn shutdown_agent_framework(&self) -> Result<()> {
        // Keep the retry owner installed across errors and cancellation of this wait.
        let framework = self.agent_facility.framework.lock().await.clone();
        if let Some(framework) = framework {
            framework.closing.cancel();
            framework
                .runtime
                .shutdown()
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
            let released = self.agent_facility.framework.lock().await.take();
            drop(released);
        }
        Ok(())
    }

    /// 排空 agent framework 的 write-behind 队列并停止 writer。
    pub(super) async fn flush_persistence(&self) -> Result<()> {
        self.store
            .sessions()
            .shutdown()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let repository = self.agent_facility.persistence.lock().await.clone();
        if let Some(repository) = repository {
            repository
                .writer()
                .flush()
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            repository
                .writer()
                .shutdown()
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            self.agent_facility.persistence.lock().await.take();
        }
        Ok(())
    }

    /// 当前尚未落库的 pending commit 数量（关机进度用）。
    pub async fn pending_persistence_commits(&self) -> usize {
        let repository = self.agent_facility.persistence.lock().await.clone();
        repository.map_or(0, |repository| repository.writer().pending_commit_count())
            + self.store.sessions().persistence().pending_commits
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

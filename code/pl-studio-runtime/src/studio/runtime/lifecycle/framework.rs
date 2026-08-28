use anyhow::{Context, Result};

use crate::studio::agent_host::{
    StudioAgentHost, StudioAgentRepository, StudioAgentRuntime, runtime_options,
};

use super::super::StudioRuntime;
use super::super::lsp_state::health;

impl StudioRuntime {
    pub(in crate::studio) async fn agent_framework(
        &self,
    ) -> Result<std::sync::Arc<StudioAgentRuntime>> {
        let mut framework = self.agent_facility.framework.lock().await;
        if let Some(runtime) = framework.as_ref() {
            return Ok(runtime.clone());
        }
        let persistence = self
            .agent_facility
            .persistence
            .lock()
            .await
            .clone()
            .context("Studio persistence writer is unavailable")?;
        let host = StudioAgentHost::new(
            persistence,
            self.store.clone(),
            self.config_runtime.clone(),
            self.external_runtimes.mcp.clone(),
            self.agent_facility.tool_manager.clone(),
            self.external_runtimes.lsp.clone(),
            self.agent_facility.interactions.clone(),
            self.task_coordinator.clone(),
            self.agent_facility.resources.clone(),
            self.agent_facility.product_events.clone(),
            self.skills.clone(),
        );
        let runtime = std::sync::Arc::new(
            StudioAgentRuntime::start(host, runtime_options())
                .await
                .map_err(|error| anyhow::anyhow!(error))?,
        );
        *framework = Some(runtime.clone());
        drop(framework);
        // 活动 Task 引用和 pending continuation 目标必须先驻留，attach 时
        // materialize 才不会跳过（wake）或破坏性失败（executor continuation）。
        let mut activation_targets = self.task_runtime.active_thread_ids().await;
        for continuation in self.store.list_pending_executor_continuations().await? {
            activation_targets.push(continuation.agent_id);
        }
        activation_targets.sort();
        activation_targets.dedup();
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
        let Some(framework) = self.agent_facility.framework.lock().await.clone() else {
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
    ) -> Result<pl_protocol::Thread> {
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
        Ok(pl_protocol::Thread::from(record))
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
                let health = health(&runtime.external_runtimes.lsp).await;
                if let Err(error) = runtime.external_runtimes.lsp_state.refresh(health).await {
                    tracing::warn!(%error, "failed to refresh LSP observed state");
                }
            }
        }));
    }

    pub(super) async fn stop_lsp_state_watcher(&self) {
        if let Some(handle) = self.external_runtimes.lsp_state_watcher.lock().await.take() {
            handle.abort();
        }
    }

    /// 淘汰超出 LRU 容量的空闲驻留 actor；淘汰前先排空 pending commits。
    pub(in crate::studio::runtime) async fn enforce_residency_limit(&self) {
        let task_pins = self
            .task_runtime
            .active_thread_ids()
            .await
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let candidates = self.residency.over_capacity(&task_pins).await;
        if candidates.is_empty() {
            return;
        }
        let Some(framework) = self.agent_facility.framework.lock().await.clone() else {
            return;
        };
        let handle = framework.handle();
        for thread_id in candidates {
            // 候选计算已排除活跃订阅和非终态 Task；这里再次检查订阅 pin，覆盖
            // 候选快照生成后到实际逐出前新建订阅的竞争。
            if self.residency.is_pinned(&thread_id) {
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
                    let root_thread_id = self
                        .agent_facility
                        .product_events
                        .thread_snapshot(&thread_id)
                        .map(|thread| thread.root_thread_id);
                    if let Some(repository) = self.agent_facility.persistence.lock().await.clone()
                        && let Err(error) = repository
                            .writer()
                            .await_durable(agent_id.as_str(), snapshot.revision)
                            .await
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
                            self.agent_facility
                                .resources
                                .evict_thread_attachments(&thread_id)
                                .await;
                            // 耐久化完成后热集合条目退回冷数据，由分页查询回源。
                            self.agent_facility
                                .product_events
                                .evict_thread_entry(&thread_id);
                            if let Some(root_thread_id) = root_thread_id {
                                let _ = self.task_runtime.evict_durable(&root_thread_id).await;
                            }
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

    /// 排空 agent framework 的 write-behind 队列并停止 writer。
    pub(super) async fn flush_persistence(&self) -> Result<()> {
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

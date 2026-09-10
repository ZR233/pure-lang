use anyhow::{Context, Result};

#[cfg(test)]
use crate::studio::agent_host::ThreadWriteBehindWriter;

use super::super::StudioRuntime;
use super::super::background_task::{self, BackgroundTask};
use super::super::lsp_state::health;

impl StudioRuntime {
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

    /// 查询路径使用的只读 repository 句柄（共享进程级 writer 的实例）。
    #[cfg(test)]
    pub(in crate::studio) async fn persistence_repository(
        &self,
    ) -> Option<ThreadWriteBehindWriter> {
        self.agent_facility.persistence.lock().await.clone()
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
        for thread_id in candidates {
            if self.residency.is_pinned(&thread_id)
                || self.agent_facility.worktrees.get(&thread_id).is_some_and(|lease| lease.state != crate::studio::agent_host::worktree_lease::WorktreeLeaseState::Cleaned) {
                continue;
            }
            match self.threads.evict_idle(&thread_id).await {
                Ok(true) => {
                    self.residency.remove(&thread_id).await;
                    self.agent_facility
                        .product_events
                        .evict_thread_entry(&thread_id);
                }
                Ok(false) => self.residency.touch(&thread_id).await,
                Err(error) => {
                    tracing::warn!(%thread_id, %error, "idle Thread eviction failed; owner retained")
                }
            }
        }
    }

    /// 显式修复缺失的 Thread actor，并恢复其 durable mailbox/wake。
    pub async fn repair_thread_runtime(&self, thread_id: &str) -> Result<()> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let _ = self.ensure_thread_owner(thread_id).await?;
        Ok(())
    }

    pub(super) async fn shutdown_agent_framework(&self) -> Result<()> {
        let thread_failures = self.threads.close_all().await;
        if !thread_failures.is_empty() {
            return Err(ThreadCloseFailures(thread_failures).into());
        }
        self.thread_observations.finish().await?;
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
                .flush()
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            repository
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
        repository.map_or(0, |repository| repository.pending_commit_count())
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

#[derive(Debug)]
struct ThreadCloseFailures(Vec<(String, crate::thread_assembler::ThreadAssemblyError)>);
impl std::fmt::Display for ThreadCloseFailures {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Thread shutdown failures")?;
        for (id, error) in &self.0 {
            write!(formatter, "; {id}: {error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for ThreadCloseFailures {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0
            .first()
            .map(|(_, error)| error as &(dyn std::error::Error + 'static))
    }
}

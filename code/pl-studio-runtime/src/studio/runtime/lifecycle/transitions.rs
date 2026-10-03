use anyhow::Result;

use crate::studio::ids::unix_seconds;
use crate::studio::{StudioRuntimeCommand, StudioRuntimeSnapshot};

use super::super::StudioRuntime;

impl StudioRuntime {
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
        let current = self.runtime_snapshot().await?;
        if current.state.is_stopped() {
            return self.runtime_snapshot().await;
        }
        let _ = self
            .runtime_state
            .apply(StudioRuntimeCommand::BeginShutdown {
                expected_revision: current.revision,
                at: unix_seconds(),
            })?;
        // Fence publication and join all non-abortable commits before closing any store.
        let catalog_shutdown = self.stop_model_catalog_probes().await;
        let shutdown = async {
            // 阶段 1：订阅方自行消费进度流后由 bridge 取消订阅。
            self.shutdown_progress
                .emit(crate::StudioShutdownProgress::StoppingSubscriptions(
                    Default::default(),
                ));
            // 阶段 2：中断所有活动 Turn 并等待 actor 收束。
            self.shutdown_progress
                .emit(crate::StudioShutdownProgress::CancellingTurns(
                    Default::default(),
                ));
            // 标题任务使用同一 runtime 生命周期；先取消并等待，避免关机后
            // 陈旧 Explorer 结果再次修改目录事实。
            self.stop_recovery_scan().await?;
            self.stop_tool_refresh().await?;
            self.stop_model_refresh().await?;
            self.title_tasks.cancel_and_wait().await;
            self.shutdown_agent_framework().await?;
            // 阶段 3：等待 write-behind 全部 pending commit 落库；完成事件必须 pending=0。
            let pending_before = self.pending_persistence_commits().await;
            self.shutdown_progress
                .emit(crate::StudioShutdownProgress::FlushingPersistence(
                    crate::FlushingPersistenceProgress::new(pending_before as u64),
                ));
            self.flush_persistence().await?;
            let pending_after = self.pending_persistence_commits().await;
            anyhow::ensure!(
                pending_after == 0,
                "normal shutdown cannot continue with {pending_after} pending persistence commits"
            );
            self.shutdown_progress
                .emit(crate::StudioShutdownProgress::FlushingPersistence(
                    crate::FlushingPersistenceProgress::new(0),
                ));
            // 阶段 4：停止协作 Agent。
            self.shutdown_progress
                .emit(crate::StudioShutdownProgress::StoppingAgents(
                    Default::default(),
                ));
            // 阶段 5：关闭 MCP。
            self.shutdown_progress
                .emit(crate::StudioShutdownProgress::StoppingMcp(
                    Default::default(),
                ));
            self.stop_mcp_startup_reconcile().await?;
            self.stop_mcp_health_watcher().await?;
            self.stop_lsp_state_watcher().await?;
            self.external_runtimes.mcp.shutdown().await;
            self.publish_mcp_stopped().await?;
            // 阶段 6：关闭 LSP。
            self.shutdown_progress
                .emit(crate::StudioShutdownProgress::StoppingLsp(
                    Default::default(),
                ));
            self.external_runtimes.lsp.shutdown().await;
            self.external_runtimes.lsp_state.stopped().await?;
            super::super::background_task::stop(&self.persistence_observer)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            let writer = self.agent_facility.persistence.lock().await.take();
            if let Some(writer) = writer {
                writer.shutdown().await?;
            }
            self.store.close().await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        // A failed catalog task must still allow the remaining owners to close.
        let shutdown = match (shutdown, catalog_shutdown) {
            (Ok(()), result) => result,
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(catalog_error)) => Err(error.context(format!(
                "model catalog shutdown also failed: {catalog_error:#}"
            ))),
        };
        // Remote processes must lose their lease even when an earlier shutdown stage fails.
        let remote_shutdown = self.ssh_manager.shutdown().await;
        let shutdown = match (shutdown, remote_shutdown) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error.into()),
            (Err(error), Err(remote_error)) => {
                Err(error.context(format!("SSH shutdown also failed: {remote_error}")))
            }
        };
        if let Err(error) = shutdown {
            let _ = self
                .runtime_state
                .apply(StudioRuntimeCommand::FailShutdown {
                    expected_revision: self.runtime_state.snapshot().revision,
                    at: unix_seconds(),
                    error: pl_protocol::StateError {
                        code: "studioShutdownFailed".to_string(),
                        message: format!("{error:#}"),
                        retryable: true,
                    },
                });
            return Err(error);
        }
        let _ = self
            .runtime_state
            .apply(StudioRuntimeCommand::FinishShutdown {
                expected_revision: self.runtime_state.snapshot().revision,
                at: unix_seconds(),
            })?;
        // 阶段 7：终态。
        self.shutdown_progress
            .emit(crate::StudioShutdownProgress::Stopped(Default::default()));
        self.instance_lock.release();
        self.runtime_snapshot().await
    }

    /// 订阅关机阶段进度；通道随 runtime 共享，并发 shutdown 共享同一次序列。
    pub async fn subscribe_shutdown_progress(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::StudioShutdownProgress> {
        self.shutdown_progress.subscribe()
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_runtime().await;
    }
}

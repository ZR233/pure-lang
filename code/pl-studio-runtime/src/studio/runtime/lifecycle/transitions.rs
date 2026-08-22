use anyhow::Result;

use crate::studio::{StudioRuntimeSnapshot, StudioRuntimeStatus};

use super::super::StudioRuntime;

impl StudioRuntime {
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
        self.publish_settings_state(settings)?;
        self.agent_facility
            .product_events
            .emit_recovery_state(self.recovery.snapshot());
        self.runtime_snapshot().await
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

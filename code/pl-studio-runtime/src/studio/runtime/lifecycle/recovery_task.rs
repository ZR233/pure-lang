//! Owned background diagnostics; frontend readiness does not wait for cold histories or Git.
use super::super::{
    StudioRuntime,
    background_task::{self, BackgroundTask},
};
use anyhow::Result;

impl StudioRuntime {
    /// Starts or retries diagnostics; concurrent requests share the current scan.
    pub async fn retry_recovery(&self) -> Result<crate::StudioRecoveryStateSnapshot> {
        let _lifecycle = self.lifecycle_lock.lock().await;
        anyhow::ensure!(
            self.runtime_state.snapshot().state.is_ready(),
            "runtime is not ready"
        );
        self.start_recovery_scan().await;
        Ok(self.read_recovery_state())
    }

    pub fn read_recovery_state(&self) -> crate::StudioRecoveryStateSnapshot {
        crate::StudioRecoveryStateSnapshot {
            state: self.recovery.state(),
        }
    }

    pub(super) async fn start_recovery_scan(&self) {
        let mut slot = self.recovery_task.lock().await;
        if slot.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        self.recovery.begin();
        self.publish_recovery();
        let runtime = self.clone();
        *slot = Some(BackgroundTask::new(tokio::spawn(async move {
            #[cfg(test)]
            let _gate = runtime.recovery_gate.lock().await;
            let mut issues = Vec::new();
            let result = async {
                let stage = crate::startup_timing::Stage::new("recover_sessions");
                runtime.append_session_recovery_issues(&mut issues).await?;
                drop(stage);
                let stage = crate::startup_timing::Stage::new("recover_worktrees");
                runtime.append_worktree_recovery_issues(&mut issues).await?;
                drop(stage);
                runtime
                    .append_unavailable_project_recovery_issues(&mut issues)
                    .await?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            match result {
                Ok(()) => {
                    issues.retain(|issue| issue.worktree.as_ref().is_none_or(|preview| {
                        runtime.agent_facility.worktrees.get(&preview.owner_thread_id)
                            .is_some_and(|lease| lease.revision == preview.lease_revision
                                && lease.state != crate::studio::agent_host::worktree_lease::WorktreeLeaseState::Cleaned)
                    }));
                    runtime.recovery.replace(issues);
                }
                Err(error) => {
                    tracing::warn!(
                        error_bytes = error.to_string().len(),
                        "background recovery failed"
                    );
                    runtime.recovery.fail(pl_protocol::StateError {
                        code: "recoveryCheckFailed".into(),
                        message: format!("{error:#}"),
                        retryable: true,
                    });
                }
            }
            runtime.publish_recovery();
        })));
    }
    fn publish_recovery(&self) {
        self.agent_facility
            .product_events
            .emit_recovery_state(self.recovery.state());
    }
    pub(super) async fn stop_recovery_scan(&self) -> Result<()> {
        background_task::stop(&self.recovery_task)
            .await
            .map_err(|error| anyhow::anyhow!("recovery task failed: {error}"))?;
        self.recovery.stop();
        self.publish_recovery();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_protocol::ObservedResourceKind;

    #[tokio::test]
    async fn blocked_background_recovery_does_not_block_first_snapshot_or_shutdown() {
        let home = tempfile::tempdir().unwrap();
        let runtime = StudioRuntime::with_options(crate::StudioRuntimeOptions {
            studio_home: Some(home.path().into()),
            host: crate::StudioHostKind::Test,
        })
        .await
        .unwrap();
        let gate = runtime.recovery_gate.lock().await;
        tokio::time::timeout(std::time::Duration::from_secs(5), runtime.start_runtime())
            .await
            .unwrap()
            .unwrap();
        let snapshot = runtime.read_state().await.unwrap();
        assert!(snapshot.runtime.state.is_ready());
        assert_eq!(
            snapshot.recovery.state.kind(),
            ObservedResourceKind::Refreshing
        );
        let revision = snapshot.recovery.state.revision();
        assert_eq!(
            runtime.retry_recovery().await.unwrap().state.revision(),
            revision
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            runtime.shutdown_runtime(),
        )
        .await
        .unwrap()
        .unwrap();
        drop(gate);
        assert!(
            runtime
                .recovery_task
                .lock()
                .await
                .as_ref()
                .unwrap()
                .is_finished()
        );
        assert!(runtime.runtime_snapshot().await.unwrap().state.is_stopped());
    }

    #[tokio::test]
    async fn background_recovery_completes_and_can_be_retried() {
        let home = tempfile::tempdir().unwrap();
        let runtime = StudioRuntime::with_options(crate::StudioRuntimeOptions {
            studio_home: Some(home.path().into()),
            host: crate::StudioHostKind::Test,
        })
        .await
        .unwrap();
        runtime.start_runtime().await.unwrap();
        background_task::finish(&runtime.recovery_task)
            .await
            .unwrap();
        assert_eq!(
            runtime.read_recovery_state().state.kind(),
            ObservedResourceKind::Ready
        );
        let before = runtime.read_recovery_state().state.revision();
        runtime.retry_recovery().await.unwrap();
        background_task::finish(&runtime.recovery_task)
            .await
            .unwrap();
        assert!(runtime.read_recovery_state().state.revision() > before);
        runtime.shutdown_runtime().await.unwrap();
    }
}

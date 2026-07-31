//! Task 产品事实信号与 executor delivery recovery 的宿主适配。

mod delivery_recovery;
mod signal;

use std::sync::Arc;

use pl_core::{
    AgentLifecycleState, AgentRuntimeError, AgentRuntimeHandle, AgentSnapshot, SessionId,
};
use tokio::sync::{Mutex, RwLock};

use crate::studio::StudioStore;
use crate::studio::task_coordinator::{TaskCoordinator, TaskRunPhase, TaskRunRecord};
use crate::{ErrorSeverity, SessionEventFact, SessionEventKind};

use super::resources::{root_agent_id, root_session_id};

/// Task durable signal adapter 与 executor delivery recovery facade。
#[derive(Clone)]
pub(in crate::studio) struct StudioContinuationService {
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    runtime: Arc<RwLock<Option<AgentRuntimeHandle>>>,
    terminal_watcher: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl StudioContinuationService {
    pub(in crate::studio) fn new(store: StudioStore, coordinator: Arc<TaskCoordinator>) -> Self {
        Self {
            store,
            coordinator,
            runtime: Default::default(),
            terminal_watcher: Default::default(),
        }
    }

    pub(in crate::studio) async fn attach(
        &self,
        runtime: AgentRuntimeHandle,
    ) -> anyhow::Result<()> {
        *self.runtime.write().await = Some(runtime.clone());
        self.start_terminal_watcher().await;
        self.reconcile_blocked_task_runtimes(&runtime).await?;
        self.replay_durable_product_signals().await?;
        self.resume_pending_delivery_recoveries().await
    }

    pub(in crate::studio) async fn detach(&self) {
        if let Some(watcher) = self.terminal_watcher.lock().await.take() {
            watcher.abort();
        }
        *self.runtime.write().await = None;
    }

    async fn start_terminal_watcher(&self) {
        if let Some(watcher) = self.terminal_watcher.lock().await.take() {
            watcher.abort();
        }
        let service = self.clone();
        let mut terminal_facts = self.coordinator.subscribe_terminal_facts();
        let watcher = tokio::spawn(async move {
            loop {
                match terminal_facts.recv().await {
                    Ok(task_run_id) => {
                        if let Err(error) = service.quiesce_if_blocked(&task_run_id).await {
                            service.fail(&task_run_id, error).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let runtime = service.runtime.read().await.clone();
                        if let Some(runtime) = runtime
                            && let Err(error) =
                                service.reconcile_blocked_task_runtimes(&runtime).await
                        {
                            tracing::warn!(
                                "failed to reconcile blocked Task runtimes after terminal fact lag: {error:#}"
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        *self.terminal_watcher.lock().await = Some(watcher);
    }

    async fn reconcile_blocked_task_runtimes(
        &self,
        runtime: &AgentRuntimeHandle,
    ) -> anyhow::Result<()> {
        let roots = runtime
            .list()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .into_iter()
            .filter_map(|snapshot| root_session_id(&snapshot.identity.id))
            .collect::<Vec<_>>();
        for session_id in roots {
            let Some(run) = self
                .store
                .find_latest_task_run_for_session(&session_id)
                .await?
            else {
                continue;
            };
            if run.phase == TaskRunPhase::Blocked
                && let Err(error) = quiesce_blocked_runtime(runtime, &run).await
            {
                self.fail(&run.id, error).await;
            }
        }
        Ok(())
    }

    async fn quiesce_if_blocked(&self, task_run_id: &str) -> anyhow::Result<()> {
        let Some(run) = self.store.read_task_run(task_run_id).await? else {
            return Ok(());
        };
        if run.phase != TaskRunPhase::Blocked {
            return Ok(());
        }
        let Some(runtime) = self.runtime.read().await.clone() else {
            return Ok(());
        };
        quiesce_blocked_runtime(&runtime, &run).await
    }

    async fn fail(&self, task_run_id: &str, error: anyhow::Error) {
        let diagnostic = format!("task continuation failed for {task_run_id}: {error:#}");
        let _ = self
            .coordinator
            .block_continuation_failure(task_run_id, diagnostic.clone())
            .await;
        if let Ok(Some(run)) = self.store.read_task_run(task_run_id).await {
            self.emit_error(&run.session_id, diagnostic);
        }
    }

    fn emit_error(&self, session_id: &str, message: String) {
        let runtime = self.runtime.clone();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            let Some(runtime) = runtime.read().await.clone() else {
                tracing::warn!("cannot record Studio continuation error before runtime attachment");
                return;
            };
            let emitted_at = crate::studio::ids::unix_seconds();
            let target = root_agent_id(&session_id);
            let session = match SessionId::new(session_id) {
                Ok(session) => session,
                Err(error) => {
                    tracing::warn!("invalid Studio continuation session: {error}");
                    return;
                }
            };
            if let Err(error) = runtime
                .record_session_facts(
                    target,
                    session,
                    vec![SessionEventFact::durable(
                        None,
                        None,
                        emitted_at,
                        SessionEventKind::ErrorOccurred {
                            message,
                            severity: ErrorSeverity::Recoverable,
                        },
                    )],
                )
                .await
            {
                tracing::warn!("failed to record Studio continuation error: {error}");
            }
        });
    }
}

async fn quiesce_blocked_runtime(
    runtime: &AgentRuntimeHandle,
    run: &TaskRunRecord,
) -> anyhow::Result<()> {
    let root = root_agent_id(&run.session_id);
    runtime.suspend_parent_continuations(root.clone());
    let root_snapshot = match runtime.snapshot(root.clone()).await {
        Ok(snapshot) => snapshot,
        Err(AgentRuntimeError::NotFound(_)) => return Ok(()),
        Err(error) => return Err(anyhow::anyhow!(error.to_string())),
    };
    cancel_root_turn(runtime, &root_snapshot).await?;
    runtime
        .quiesce_parent_wait(root.clone())
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    close_task_children(runtime, &root).await?;
    if root_snapshot.active_turn_id.is_some() {
        runtime
            .wait_timeout(root, std::time::Duration::from_secs(10))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    Ok(())
}

async fn cancel_root_turn(
    runtime: &AgentRuntimeHandle,
    snapshot: &AgentSnapshot,
) -> anyhow::Result<()> {
    let Some(turn_id) = snapshot.active_turn_id.clone() else {
        return Ok(());
    };
    match runtime
        .cancel_turn(snapshot.identity.id.clone(), turn_id)
        .await
    {
        Ok(())
        | Err(AgentRuntimeError::NoActiveTurn(_))
        | Err(AgentRuntimeError::TurnMismatch { .. }) => {}
        Err(error) => return Err(anyhow::anyhow!(error.to_string())),
    }
    Ok(())
}

async fn close_task_children(
    runtime: &AgentRuntimeHandle,
    root: &pl_core::AgentId,
) -> anyhow::Result<()> {
    let children = runtime
        .list()
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .into_iter()
        .filter(|snapshot| snapshot.identity.parent_id.as_ref() == Some(root))
        .filter(|snapshot| {
            !matches!(
                snapshot.lifecycle,
                AgentLifecycleState::Closing | AgentLifecycleState::Closed
            )
        })
        .map(|snapshot| snapshot.identity.id)
        .collect::<Vec<_>>();
    for child in children {
        runtime
            .close(child)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    Ok(())
}

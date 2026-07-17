use std::collections::BTreeSet;
use std::sync::Arc;

use crate::ErrorSeverity;
use pl_core::{AgentRuntimeHandle, AgentSubmitRequest, InputDelivery, SessionId, TurnOutcomeKind};
use pl_trace::AgentEvent;
use tokio::sync::{Mutex, RwLock};

use crate::studio::task_coordinator::{
    StudioAgentTerminalChange, TaskContinuationResolution, TaskCoordinator,
    TerminalAgentStateRecording,
};
use crate::studio::{StudioEventRuntime, StudioStore};

use super::resources::root_agent_id;

/// Studio 触发任务续轮的产品原因，仅用于 durable 输入诊断和去重。
#[derive(Debug, Clone, Copy)]
pub(in crate::studio) enum StudioContinuationReason {
    Recovery,
    AgentTerminal,
    ReviewReturned,
    MergeConflict,
    MergeCompleted,
}

impl StudioContinuationReason {
    fn label(self) -> &'static str {
        match self {
            Self::Recovery => "recovery",
            Self::AgentTerminal => "agentTerminal",
            Self::ReviewReturned => "reviewReturned",
            Self::MergeConflict => "mergeConflict",
            Self::MergeCompleted => "mergeCompleted",
        }
    }
}

/// 将 TaskCoordinator 的 durable 事实转换为 PL runtime FIFO 输入。
///
/// 本服务不维护第二套 active-turn 或 pending queue；进程内集合只合并同一时刻的
/// 重复触发，跨重启去重以 repository 中的 live turn 元数据为准。
#[derive(Clone)]
pub(in crate::studio) struct StudioContinuationService {
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    events: StudioEventRuntime,
    runtime: Arc<RwLock<Option<AgentRuntimeHandle>>>,
    dispatching: Arc<Mutex<BTreeSet<String>>>,
}

impl StudioContinuationService {
    pub(in crate::studio) fn new(
        store: StudioStore,
        coordinator: Arc<TaskCoordinator>,
        events: StudioEventRuntime,
    ) -> Self {
        Self {
            store,
            coordinator,
            events,
            runtime: Default::default(),
            dispatching: Default::default(),
        }
    }

    pub(in crate::studio) async fn attach(&self, runtime: AgentRuntimeHandle) {
        *self.runtime.write().await = Some(runtime);
    }

    pub(in crate::studio) async fn detach(&self) {
        *self.runtime.write().await = None;
    }

    pub(in crate::studio) fn request(&self, task_run_id: String, reason: StudioContinuationReason) {
        let service = self.clone();
        tokio::spawn(async move {
            if let Err(error) = service.dispatch(task_run_id.clone(), reason).await {
                service.fail(&task_run_id, error).await;
            }
        });
    }

    pub(super) async fn record_child_terminal(
        &self,
        studio_session_id: &str,
        agent_id: &str,
        role: &str,
        outcome: TurnOutcomeKind,
        reason: Option<String>,
    ) {
        let change = StudioAgentTerminalChange {
            agent_id: agent_id.to_string(),
            role: role.to_string(),
            outcome,
            summary: reason.clone(),
            error: matches!(
                outcome,
                TurnOutcomeKind::Failed | TurnOutcomeKind::BudgetLimited
            )
            .then_some(reason)
            .flatten(),
        };
        match self
            .coordinator
            .record_terminal_agent_state(studio_session_id, &change)
            .await
        {
            Ok(TerminalAgentStateRecording::Changed { task_run_id, .. }) => {
                let reason = if role == "reviewer" {
                    StudioContinuationReason::ReviewReturned
                } else {
                    StudioContinuationReason::AgentTerminal
                };
                self.request(task_run_id, reason);
            }
            Ok(
                TerminalAgentStateRecording::Projected(_)
                | TerminalAgentStateRecording::Unhandled
                | TerminalAgentStateRecording::Suppressed,
            ) => {}
            Err(error) => {
                let diagnostic =
                    format!("terminal agent state persistence failed for {agent_id}: {error}");
                let _ = self
                    .coordinator
                    .block_terminal_persistence_failure(studio_session_id, &error.to_string())
                    .await;
                let _ = self
                    .events
                    .emit_agent_event(
                        studio_session_id,
                        AgentEvent::Error {
                            message: diagnostic,
                            severity: ErrorSeverity::Recoverable,
                        },
                    )
                    .await;
            }
        }
    }

    pub(super) async fn request_merge_follow_up(&self, studio_session_id: &str) {
        for (claim, reason) in [
            (
                self.store
                    .claim_merge_conflict_continuation(studio_session_id)
                    .await,
                StudioContinuationReason::MergeConflict,
            ),
            (
                self.store
                    .claim_merge_completion_continuation(studio_session_id)
                    .await,
                StudioContinuationReason::MergeCompleted,
            ),
        ] {
            match claim {
                Ok(Some(task_run_id)) => self.request(task_run_id, reason),
                Ok(None) => {}
                Err(error) => {
                    let diagnostic = format!("task continuation claim failed: {error}");
                    let _ = self
                        .events
                        .emit_agent_event(
                            studio_session_id,
                            AgentEvent::Error {
                                message: diagnostic,
                                severity: ErrorSeverity::Recoverable,
                            },
                        )
                        .await;
                }
            }
        }
    }

    async fn dispatch(
        &self,
        task_run_id: String,
        reason: StudioContinuationReason,
    ) -> anyhow::Result<()> {
        {
            let mut dispatching = self.dispatching.lock().await;
            if !dispatching.insert(task_run_id.clone()) {
                return Ok(());
            }
        }
        let result = self.dispatch_once(&task_run_id, reason).await;
        self.dispatching.lock().await.remove(&task_run_id);
        result
    }

    async fn dispatch_once(
        &self,
        task_run_id: &str,
        reason: StudioContinuationReason,
    ) -> anyhow::Result<()> {
        if self.store.has_queued_task_continuation(task_run_id).await? {
            return Ok(());
        }
        let snapshot = match self
            .store
            .load_task_continuation_resolution(task_run_id)
            .await?
        {
            TaskContinuationResolution::Active(snapshot) => *snapshot,
            TaskContinuationResolution::Terminal(_) => return Ok(()),
        };
        let session = self
            .store
            .read_session(&snapshot.run.session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task continuation session not found"))?;
        if session.mode != "task" {
            anyhow::bail!("task continuation session is not in task mode");
        }
        let runtime = self
            .runtime
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Studio agent runtime is not attached"))?;
        let session_id = SessionId::new(snapshot.run.session_id.clone())?;
        runtime
            .submit(
                root_agent_id(&snapshot.run.session_id),
                AgentSubmitRequest::start(session_id, snapshot.render_prompt()?)
                    .with_delivery(InputDelivery::Start)
                    .with_metadata(serde_json::json!({
                        "taskRunId": task_run_id,
                        "continuationReason": reason.label(),
                        "attachmentIds": [],
                        "userPrompt": {
                            "visiblePrompt": "继续任务",
                            "synthetic": true,
                            "ignored": true,
                        },
                        "historyPolicy": "ephemeral",
                    })),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(())
    }

    async fn fail(&self, task_run_id: &str, error: anyhow::Error) {
        let diagnostic = format!("task continuation failed for {task_run_id}: {error:#}");
        let _ = self
            .coordinator
            .block_continuation_failure(task_run_id, diagnostic.clone())
            .await;
        if let Ok(Some(run)) = self.store.read_task_run(task_run_id).await {
            let _ = self
                .events
                .emit_agent_event(
                    &run.session_id,
                    AgentEvent::Error {
                        message: diagnostic,
                        severity: ErrorSeverity::Recoverable,
                    },
                )
                .await;
        }
    }
}

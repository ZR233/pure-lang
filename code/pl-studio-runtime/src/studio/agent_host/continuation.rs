use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{ErrorSeverity, SessionEventFact, SessionEventKind};
use pl_core::{
    AgentCurrentSessionSubmitRequest, AgentRuntimeHandle, AgentSubmitRequest, InputDelivery,
    SessionId, TurnOutcomeKind,
};
use tokio::sync::{Mutex, RwLock};

use crate::studio::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, DeliveryRecoveryClaim, DeliveryRecoveryDispatch, DeliveryRecoveryNeed,
    StudioAgentTerminalChange, TaskContinuationResolution, TaskCoordinator,
    TerminalAgentStateRecording,
};

use super::resources::root_agent_id;

/// Studio 触发任务续轮的产品原因，仅用于 durable 输入诊断和去重。
#[derive(Debug, Clone, Copy)]
pub(in crate::studio) enum StudioContinuationReason {
    Recovery,
    AgentTerminal,
    DeliveryCompleted,
    ReviewReturned,
    MergeConflict,
    MergeCompleted,
}

impl StudioContinuationReason {
    fn label(self) -> &'static str {
        match self {
            Self::Recovery => "recovery",
            Self::AgentTerminal => "agentTerminal",
            Self::DeliveryCompleted => "deliveryCompleted",
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
    runtime: Arc<RwLock<Option<AgentRuntimeHandle>>>,
    dispatching: Arc<Mutex<BTreeSet<String>>>,
}

impl StudioContinuationService {
    pub(in crate::studio) fn new(store: StudioStore, coordinator: Arc<TaskCoordinator>) -> Self {
        Self {
            store,
            coordinator,
            runtime: Default::default(),
            dispatching: Default::default(),
        }
    }

    pub(in crate::studio) async fn attach(
        &self,
        runtime: AgentRuntimeHandle,
    ) -> anyhow::Result<()> {
        *self.runtime.write().await = Some(runtime);
        self.resume_pending_delivery_recoveries().await
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
        _child_session_id: &str,
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
            Ok(TerminalAgentStateRecording::Changed {
                task_run_id,
                projection,
            }) if role == "executor"
                && projection.status == AgentOutcomeStatus::WaitingForDelivery =>
            {
                self.recover_executor_delivery(studio_session_id, &task_run_id, agent_id)
                    .await;
            }
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
                self.emit_error(studio_session_id, diagnostic);
            }
        }
    }

    async fn recover_executor_delivery(
        &self,
        studio_session_id: &str,
        task_run_id: &str,
        agent_id: &str,
    ) {
        let dispatch_key = format!("delivery-recovery:{task_run_id}:{agent_id}");
        {
            let mut dispatching = self.dispatching.lock().await;
            if !dispatching.insert(dispatch_key.clone()) {
                return;
            }
        }
        self.recover_executor_delivery_once(studio_session_id, task_run_id, agent_id)
            .await;
        self.dispatching.lock().await.remove(&dispatch_key);
    }

    async fn recover_executor_delivery_once(
        &self,
        studio_session_id: &str,
        task_run_id: &str,
        agent_id: &str,
    ) {
        match self
            .coordinator
            .inspect_delivery_recovery_need(task_run_id, agent_id)
            .await
        {
            Ok(DeliveryRecoveryNeed::NoDelivery) => {
                if let Err(error) = self
                    .store
                    .fail_executor_without_delivery(task_run_id, agent_id)
                    .await
                {
                    self.fail(task_run_id, error).await;
                    return;
                }
                self.request(
                    task_run_id.to_string(),
                    StudioContinuationReason::AgentTerminal,
                );
                return;
            }
            Ok(DeliveryRecoveryNeed::Recoverable) => {}
            Err(error) => {
                self.fail(task_run_id, error).await;
                return;
            }
        }
        let claim = match self
            .store
            .claim_delivery_recovery(task_run_id, agent_id)
            .await
        {
            Ok(Some(claim)) => claim,
            Ok(None) => {
                self.request(
                    task_run_id.to_string(),
                    StudioContinuationReason::AgentTerminal,
                );
                return;
            }
            Err(error) => {
                let diagnostic =
                    format!("executor delivery recovery claim failed for {agent_id}: {error:#}");
                self.emit_error(studio_session_id, diagnostic.clone());
                self.fail(task_run_id, anyhow::anyhow!(diagnostic)).await;
                return;
            }
        };
        match self.store.delivery_recovery_dispatch(&claim).await {
            Ok(Some(DeliveryRecoveryDispatch::Pending)) => return,
            Ok(Some(DeliveryRecoveryDispatch::Terminal { outcome, reason })) => {
                self.replay_delivery_recovery_terminal(
                    studio_session_id,
                    task_run_id,
                    agent_id,
                    outcome,
                    reason,
                )
                .await;
                return;
            }
            Ok(None) => {}
            Err(error) => {
                let diagnostic = format!(
                    "executor delivery recovery dispatch lookup failed for {agent_id}: {error:#}"
                );
                self.emit_error(studio_session_id, diagnostic.clone());
                self.fail(task_run_id, anyhow::anyhow!(diagnostic)).await;
                return;
            }
        }
        if let Err(error) = self.submit_delivery_recovery(&claim).await {
            let diagnostic =
                format!("executor delivery recovery submit failed for {agent_id}: {error:#}");
            if let Err(store_error) = self.store.fail_delivery_recovery(&claim, &diagnostic).await {
                self.fail(
                    task_run_id,
                    anyhow::anyhow!("{diagnostic}; durable failure update failed: {store_error:#}"),
                )
                .await;
                return;
            }
            self.emit_error(studio_session_id, diagnostic);
            self.request(
                task_run_id.to_string(),
                StudioContinuationReason::AgentTerminal,
            );
        }
    }

    async fn replay_delivery_recovery_terminal(
        &self,
        studio_session_id: &str,
        task_run_id: &str,
        agent_id: &str,
        outcome: TurnOutcomeKind,
        reason: Option<String>,
    ) {
        let change = StudioAgentTerminalChange {
            agent_id: agent_id.to_string(),
            role: "executor".to_string(),
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
            Ok(
                TerminalAgentStateRecording::Changed { .. }
                | TerminalAgentStateRecording::Projected(_),
            ) => self.request(
                task_run_id.to_string(),
                StudioContinuationReason::AgentTerminal,
            ),
            Ok(
                TerminalAgentStateRecording::Unhandled | TerminalAgentStateRecording::Suppressed,
            ) => {}
            Err(error) => {
                let diagnostic = format!(
                    "replayed delivery recovery terminal persistence failed for {agent_id}: {error}"
                );
                let _ = self
                    .coordinator
                    .block_terminal_persistence_failure(studio_session_id, &error.to_string())
                    .await;
                self.emit_error(studio_session_id, diagnostic);
            }
        }
    }

    async fn submit_delivery_recovery(&self, claim: &DeliveryRecoveryClaim) -> anyhow::Result<()> {
        let runtime = self
            .runtime
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Studio agent runtime is not attached"))?;
        runtime
            .submit_current_session(
                pl_core::AgentId::new(claim.agent_id.clone())?,
                AgentCurrentSessionSubmitRequest::start(
                    "交付合同尚未完成。请检查当前 executor worktree：运行与改动相关的实际验证；\
                     如有未提交修改先提交；确认 HEAD 已相对 base 推进且工作区干净；\
                     然后必须调用 submit_delivery，提供真实 headCommit 与 verificationSummary。\
                     不要只返回文字总结，也不要伪造验证结果。",
                )
                .with_delivery(InputDelivery::Start)
                .with_metadata(serde_json::json!({
                    "taskRunId": claim.task_run_id,
                    "workUnitId": claim.work_unit_id,
                    "deliveryRecoveryCount": claim.recovery_count,
                    "deliveryRecoveryDispatchId": claim.dispatch_id(),
                    "continuationReason": "deliveryRecovery",
                    "attachmentIds": [],
                    "userPrompt": {
                        "visiblePrompt": "恢复未完成的 executor 交付",
                        "synthetic": true,
                        "ignored": true,
                    },
                })),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(())
    }

    async fn resume_pending_delivery_recoveries(&self) -> anyhow::Result<()> {
        for run in self.store.list_active_task_runs().await? {
            for outcome in self.store.list_agent_outcomes(&run.id).await? {
                if outcome.role == "executor"
                    && outcome.status == AgentOutcomeStatus::WaitingForDelivery
                    && outcome.delivery.is_none()
                {
                    self.recover_executor_delivery(&run.session_id, &run.id, &outcome.agent_id)
                        .await;
                }
            }
        }
        Ok(())
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
                    self.emit_error(studio_session_id, diagnostic);
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
        if self.store.has_live_task_continuation(task_run_id).await? {
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

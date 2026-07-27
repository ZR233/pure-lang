use pl_core::{AgentCurrentSessionSubmitRequest, InputDelivery, TurnOutcomeKind};

use crate::studio::task_coordinator::{
    AgentOutcomeStatus, DeliveryRecoveryClaim, DeliveryRecoveryDispatch, DeliveryRecoveryNeed,
    StudioAgentTerminalChange, TerminalAgentStateRecording,
};

use super::StudioContinuationService;

impl StudioContinuationService {
    pub(super) async fn recover_executor_delivery(
        &self,
        studio_session_id: &str,
        task_run_id: &str,
        agent_id: &str,
    ) {
        self.recover_executor_delivery_once(studio_session_id, task_run_id, agent_id)
            .await;
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
                self.publish_outcome_signal(task_run_id, agent_id, "deliveryRecoveryFailed")
                    .await;
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
            Ok(None) => return,
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
            self.publish_outcome_signal(task_run_id, agent_id, "deliveryRecoveryFailed")
                .await;
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
            Ok(TerminalAgentStateRecording::Changed {
                outcome_id,
                projection,
                ..
            }) => {
                self.publish_product_signal(
                    task_run_id,
                    agent_id,
                    format!("agent-outcome:{outcome_id}"),
                    "deliveryRecoveryTerminal",
                    projection.error.or(projection.summary),
                )
                .await;
            }
            Ok(TerminalAgentStateRecording::Projected(_)) => {}
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

    pub(super) async fn resume_pending_delivery_recoveries(&self) -> anyhow::Result<()> {
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
}

use pl_core::{AgentId, TurnOutcomeKind};

use crate::studio::task_coordinator::{
    AgentOutcomeStatus, MergeStatus, ReviewVerdict, StudioAgentTerminalChange,
    TerminalAgentStateRecording,
};

use super::StudioContinuationService;
use crate::studio::agent_host::resources::root_agent_id;

impl StudioContinuationService {
    pub(in crate::studio) fn request_recovery(&self, task_run_id: String) {
        let service = self.clone();
        tokio::spawn(async move {
            let run = match service.store.read_task_run(&task_run_id).await {
                Ok(Some(run)) => run,
                Ok(None) => return,
                Err(error) => {
                    service.fail(&task_run_id, error).await;
                    return;
                }
            };
            let outcome = match service.store.list_agent_outcomes(&task_run_id).await {
                Ok(outcomes) => outcomes.into_iter().max_by(|left, right| {
                    left.updated_at
                        .cmp(&right.updated_at)
                        .then_with(|| left.id.cmp(&right.id))
                }),
                Err(error) => {
                    service.fail(&task_run_id, error).await;
                    return;
                }
            };
            let Some(outcome) = outcome else {
                return;
            };
            service
                .publish_product_signal(
                    &task_run_id,
                    &outcome.agent_id,
                    format!("task-recovery:{}:{}", run.id, run.updated_at),
                    "recovery",
                    run.status_message,
                )
                .await;
        });
    }

    pub(super) async fn replay_durable_product_signals(&self) -> anyhow::Result<()> {
        for run in self.store.list_active_task_runs().await? {
            let outcomes = self.store.list_agent_outcomes(&run.id).await?;
            for outcome in &outcomes {
                if outcome.delivery.is_some() {
                    self.publish_product_signal(
                        &run.id,
                        &outcome.agent_id,
                        format!("delivery:{}", outcome.id),
                        "deliveryCompleted",
                        outcome.summary.clone(),
                    )
                    .await;
                } else if !matches!(
                    outcome.status,
                    AgentOutcomeStatus::Queued
                        | AgentOutcomeStatus::Running
                        | AgentOutcomeStatus::WaitingForDelivery
                ) {
                    self.publish_product_signal(
                        &run.id,
                        &outcome.agent_id,
                        format!("agent-outcome:{}", outcome.id),
                        "agentTerminal",
                        outcome.error.clone().or_else(|| outcome.summary.clone()),
                    )
                    .await;
                }
            }

            for round in self.store.list_review_rounds(&run.id).await? {
                if matches!(
                    round.verdict,
                    ReviewVerdict::Pending | ReviewVerdict::Failed
                ) {
                    continue;
                }
                let Some(reviewer_agent_id) = round.reviewer_agent_id else {
                    continue;
                };
                self.publish_product_signal(
                    &run.id,
                    &reviewer_agent_id,
                    format!("review:{}", round.id),
                    "reviewReturned",
                    round.summary,
                )
                .await;
            }

            let merges = self.store.list_merge_records(&run.id).await?;
            for merge in merges.iter().filter(|merge| {
                merge.status == MergeStatus::Merged
                    && merge.evidence.as_ref().is_some_and(|evidence| {
                        evidence.merge_commit.is_some()
                            && evidence.merge_completion_continuation_requested
                    })
            }) {
                self.publish_product_signal(
                    &run.id,
                    &merge.agent_id,
                    format!("merge-completion:{}", merge.id),
                    "mergeCompleted",
                    None,
                )
                .await;
            }
            for merge in merges.iter().filter(|merge| {
                merge.status == MergeStatus::Conflicted
                    && merge
                        .evidence
                        .as_ref()
                        .is_some_and(|evidence| evidence.conflict_continuation_requested)
            }) {
                self.publish_product_signal(
                    &run.id,
                    &merge.agent_id,
                    format!("merge-conflict:{}", merge.id),
                    "mergeConflict",
                    None,
                )
                .await;
            }
            self.request_merge_follow_up(&run.session_id).await;

            if let Some(outcome) = outcomes.into_iter().max_by(|left, right| {
                left.updated_at
                    .cmp(&right.updated_at)
                    .then_with(|| left.id.cmp(&right.id))
            }) {
                self.publish_product_signal(
                    &run.id,
                    &outcome.agent_id,
                    format!("task-recovery:{}:{}", run.id, run.updated_at),
                    "recovery",
                    run.status_message,
                )
                .await;
            }
        }
        Ok(())
    }

    pub(in crate::studio) async fn record_child_terminal(
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
                ..
            }) if role == "executor"
                && projection.status == AgentOutcomeStatus::WaitingForDelivery =>
            {
                self.recover_executor_delivery(studio_session_id, &task_run_id, agent_id)
                    .await;
            }
            Ok(TerminalAgentStateRecording::Changed {
                task_run_id,
                outcome_id,
                projection,
            }) => {
                if projection.status != AgentOutcomeStatus::Completed {
                    self.publish_product_signal(
                        &task_run_id,
                        agent_id,
                        format!("agent-outcome:{outcome_id}"),
                        "agentTerminal",
                        projection.error.or(projection.summary),
                    )
                    .await;
                }
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

    pub(super) async fn publish_outcome_signal(
        &self,
        task_run_id: &str,
        agent_id: &str,
        phase: &str,
    ) {
        let outcome = match self.store.list_agent_outcomes(task_run_id).await {
            Ok(outcomes) => outcomes
                .into_iter()
                .find(|outcome| outcome.agent_id == agent_id),
            Err(error) => {
                self.fail(task_run_id, error).await;
                return;
            }
        };
        let Some(outcome) = outcome else {
            self.fail(
                task_run_id,
                anyhow::anyhow!("durable agent outcome not found for product signal"),
            )
            .await;
            return;
        };
        self.publish_product_signal(
            task_run_id,
            agent_id,
            format!("agent-outcome:{}", outcome.id),
            phase,
            outcome.error.or(outcome.summary),
        )
        .await;
    }

    pub(super) async fn publish_product_signal(
        &self,
        task_run_id: &str,
        agent_id: &str,
        signal_id: String,
        phase: &str,
        summary: Option<String>,
    ) {
        let runtime = self.runtime.read().await.clone();
        let Some(runtime) = runtime else {
            tracing::warn!(
                task_run_id,
                agent_id,
                "cannot publish Task product signal before runtime attachment"
            );
            return;
        };
        let run = match self.store.read_task_run(task_run_id).await {
            Ok(Some(run)) => run,
            Ok(None) => {
                tracing::warn!(task_run_id, "Task product signal run is absent");
                return;
            }
            Err(error) => {
                tracing::warn!(task_run_id, %error, "failed to read Task run for product signal");
                return;
            }
        };
        let Ok(agent_id) = AgentId::new(agent_id.to_string()) else {
            tracing::warn!(agent_id, "Task product signal has invalid agent id");
            return;
        };
        if let Err(error) = runtime.publish_product_phase(
            root_agent_id(&run.session_id),
            agent_id,
            signal_id,
            phase.to_string(),
            summary,
        ) {
            tracing::warn!(task_run_id, %error, "failed to publish Task product signal");
        }
    }

    pub(in crate::studio) async fn request_merge_follow_up(&self, studio_session_id: &str) {
        match self
            .store
            .claim_merge_conflict_continuation(studio_session_id)
            .await
        {
            Ok(Some(claim)) => {
                self.publish_product_signal(
                    &claim.task_run_id,
                    &claim.agent_id,
                    claim.signal_id,
                    "mergeConflict",
                    None,
                )
                .await;
            }
            Ok(None) => {}
            Err(error) => {
                let diagnostic = format!("task continuation claim failed: {error}");
                self.emit_error(studio_session_id, diagnostic);
            }
        }
        match self
            .store
            .claim_merge_completion_continuation(studio_session_id)
            .await
        {
            Ok(claims) => {
                for claim in claims {
                    self.publish_product_signal(
                        &claim.task_run_id,
                        &claim.agent_id,
                        claim.signal_id,
                        "mergeCompleted",
                        None,
                    )
                    .await;
                }
            }
            Err(error) => {
                let diagnostic = format!("task continuation claim failed: {error}");
                self.emit_error(studio_session_id, diagnostic);
            }
        }
    }
}

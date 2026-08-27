//! Task 计划确认：先推进 TaskRun，再以稳定 continuation 记录用户答复并开启新的计划者执行。

use anyhow::{Context, Result};

use crate::studio::InteractionEmitter;
use crate::{
    InteractionContent, InteractionRequest, InteractionResolution, InteractionStatus,
    PlanConfirmationResolution,
};
use pl_protocol::studio::{StudioError, StudioErrorCode};

use super::{StudioResolveInteractionResponse, StudioRuntime};

const EDIT_DOCUMENTS_MESSAGE: &str = "用户已确认当前计划。任务现在处于 editingDocuments。先读取 task_status，按已确认计划补充设计、说明和实施边界；完成后调用 task_transition.finishDocumentEditing，不能直接派发执行者。";
const REVISE_PLAN_PREFIX: &str = "用户要求修改当前计划。任务已回到 planning。结合原计划和下面的调整要求继续探索并生成完整的新计划；完成后调用 task_transition.submitPlan，不能开始实施。";

impl StudioRuntime {
    pub(super) async fn resolve_plan_confirmation(
        &self,
        interaction_id: String,
        current: InteractionRequest,
        resolution: InteractionResolution,
        _emitter: InteractionEmitter,
    ) -> Result<StudioResolveInteractionResponse> {
        let thread_id = current.scope.thread_id.clone();
        if current.status() != InteractionStatus::Pending {
            return Err(plan_confirmation_conflict());
        }
        let InteractionContent::PlanConfirmation(plan) = &current.content else {
            unreachable!("plan confirmation resolution was validated before resolving");
        };
        let original_plan = plan.content().trim().to_string();
        let InteractionResolution::PlanConfirmation(payload) = resolution else {
            unreachable!("resolution kind was validated before resolving");
        };
        if payload.decision == PlanConfirmationResolution::RevisePlan {
            payload
                .content
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .context("plan adjustment is empty")?;
        }

        let aggregate = self.task_runtime.aggregate(&thread_id).await;
        let persisted_task = self
            .store
            .find_latest_task_run_for_root_thread(&thread_id)
            .await?;
        if aggregate
            .as_ref()
            .is_some_and(|aggregate| aggregate.facts.run.kind().is_terminal())
            || persisted_task.is_some_and(|task| task.kind().is_terminal())
        {
            return Err(plan_confirmation_conflict());
        }
        let aggregate = aggregate.context("plan confirmation Task aggregate is not resident")?;
        let task_command = match payload.decision {
            PlanConfirmationResolution::Confirm => {
                crate::studio::task_coordinator::TaskCommand::ConfirmPlan {
                    plan_revision: aggregate
                        .facts
                        .run
                        .plan
                        .as_ref()
                        .context("pending confirmation TaskRun has no plan")?
                        .revision,
                }
            }
            PlanConfirmationResolution::RevisePlan => {
                crate::studio::task_coordinator::TaskCommand::RequestPlanRevision
            }
        };
        let mut resolved = current.clone();
        let now = crate::studio::unix_seconds();
        let interaction_decision = resolved
            .decide(crate::InteractionCommand::ResolvePlanConfirmation(
                crate::ResolvePlanConfirmation {
                    interaction_id: resolved.interaction_id.clone(),
                    expected_revision: resolved.revision,
                    operation_id: format!("resolve:{}", resolved.interaction_id),
                    resolved_at: now,
                    decision: payload.decision,
                    content: payload.content.clone(),
                    reason: payload.reason.clone(),
                },
            ))
            .map_err(|_| plan_confirmation_conflict())?;
        resolved.apply(interaction_decision, now);
        let run = self
            .task_runtime
            .apply_run_command(
                &thread_id,
                aggregate.facts.run.revision,
                aggregate.facts.run.generation(),
                task_command,
            )
            .await
            .map_err(|_| plan_confirmation_conflict())?;
        let message = match payload.decision {
            PlanConfirmationResolution::Confirm => EDIT_DOCUMENTS_MESSAGE.to_string(),
            PlanConfirmationResolution::RevisePlan => format!(
                "{REVISE_PLAN_PREFIX}\n\n## 原计划\n\n{}\n\n## 用户调整要求\n\n{}",
                original_plan,
                payload.content.as_deref().unwrap_or_default().trim()
            ),
        };
        let (handle, owner) = self.ensure_thread_agent(&thread_id).await?;
        let mail_id = pl_core::AgentInteractionContinuationRequest::stable_mail_id(&interaction_id);
        if let Err(error) = handle
            .submit_interaction_continuation(
                owner,
                pl_core::AgentInteractionContinuationRequest::new(
                    resolved.clone(),
                    pl_core::AgentCurrentSessionSubmitRequest::start(message)
                        .with_presentation(pl_core::MailboxPresentation::Hidden)
                        .with_mail_id(mail_id)
                        .with_metadata(serde_json::json!({
                            "interactionResolutionId": interaction_id,
                            "interactionKind": "planConfirmation",
                            "taskRunId": run.id,
                        })),
                ),
            )
            .await
        {
            self.task_coordinator
                .block_continuation_failure(
                    &run.id,
                    format!("plan response continuation failed: {error}"),
                )
                .await?;
            return Err(anyhow::anyhow!(error));
        }

        Ok(StudioResolveInteractionResponse {
            thread_id,
            interaction: resolved,
        })
    }
}

fn plan_confirmation_conflict() -> anyhow::Error {
    StudioError::new(
        StudioErrorCode::Conflict,
        "This plan confirmation is no longer current",
        false,
    )
    .into()
}

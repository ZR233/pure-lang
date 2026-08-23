//! Task 计划确认：用户答复与 TaskRun 状态在同一事务提交，再开启新的计划者执行。

use anyhow::{Context, Result};

use crate::studio::InteractionEmitter;
use crate::{
    InteractionContent, InteractionRequest, InteractionResolution, InteractionStatus,
    PlanConfirmationResolution,
};

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
            return Ok(StudioResolveInteractionResponse {
                thread_id,
                interaction: current,
                threads: Vec::new(),
            });
        }
        let InteractionContent::PlanConfirmation(plan) = &current.content else {
            unreachable!("plan confirmation resolution was validated before resolving");
        };
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

        let (run, resolved) = self
            .store
            .resolve_task_plan_confirmation(&interaction_id, payload.clone())
            .await?;
        let message = match payload.decision {
            PlanConfirmationResolution::Confirm => EDIT_DOCUMENTS_MESSAGE.to_string(),
            PlanConfirmationResolution::RevisePlan => format!(
                "{REVISE_PLAN_PREFIX}\n\n## 原计划\n\n{}\n\n## 用户调整要求\n\n{}",
                plan.content().trim(),
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
                            "attachmentIds": [],
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

        let threads = if let Some(thread) = self.store.read_thread(&thread_id).await? {
            self.store.list_root_threads(&thread.project_id).await?
        } else {
            Vec::new()
        };
        Ok(StudioResolveInteractionResponse {
            thread_id,
            interaction: resolved,
            threads,
        })
    }
}

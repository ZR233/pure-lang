use crate::StudioMode;
use crate::studio::InteractionEmitter;
use crate::{
    InteractionPayload, InteractionRequest, InteractionResolution, InteractionStatus,
    PlanConfirmationResolution, PlanLifecycleState,
};
use anyhow::{Context, Result, bail};

use super::{
    StudioPlanImplementationLifecycle, StudioResolveInteractionResponse, StudioRuntime,
    StudioSubmitPromptOptions, StudioSubmitPromptRequest,
};

const IMPLEMENT_PLAN_CURRENT_THREAD_PREFIX: &str = "A previous agent produced the plan below to accomplish the user's task. Implement the plan in the current Thread. Treat the plan as the source of user intent, re-read files as needed, and carry the work through implementation and verification.";
const CONTINUE_PLANNING_PREFIX: &str = "用户对当前计划提交了调整要求。请结合原计划继续规划，只修订计划，不要开始实施。完成调整后必须再次调用 plan_exit，生成新的待确认计划。";

impl StudioRuntime {
    pub(super) async fn resolve_plan_confirmation(
        &self,
        interaction_id: String,
        current: InteractionRequest,
        resolution: InteractionResolution,
        emitter: InteractionEmitter,
    ) -> Result<StudioResolveInteractionResponse> {
        let thread_id = current.scope.thread_id.clone();
        let InteractionPayload::PlanConfirmation { plan_id, content } = &current.payload else {
            unreachable!("plan confirmation resolution was validated before resolving");
        };
        let InteractionResolution::PlanConfirmation {
            decision,
            content: resolution_content,
            reason,
        } = resolution
        else {
            unreachable!("resolution kind was validated before resolving");
        };

        if current.status != InteractionStatus::Pending {
            return Ok(StudioResolveInteractionResponse {
                thread_id,
                interaction: current,
                threads: Vec::new(),
            });
        }

        let resolved = match decision {
            PlanConfirmationResolution::ImplementFreshContext => {
                let plan_content = resolution_content
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| content.clone())
                    .trim()
                    .to_string();
                if plan_content.is_empty() {
                    bail!("plan content is empty");
                }
                let thread = self
                    .store
                    .read_thread(&thread_id)
                    .await?
                    .context("Task Thread not found")?;
                if thread.mode != StudioMode::Task.label() {
                    bail!("plan implementation requires a Task mode Thread");
                }
                let project = self
                    .store
                    .read_project(&thread.project_id)
                    .await?
                    .context("task project not found")?;
                let run = self
                    .task_coordinator
                    .start_confirmed_task(&thread_id, &plan_content, &project.path)
                    .await?;
                let started = async {
                    let resolved = self
                        .interactions
                        .resolve(
                            &interaction_id,
                            InteractionResolution::PlanConfirmation {
                                decision: PlanConfirmationResolution::ImplementFreshContext,
                                content: resolution_content,
                                reason: reason.clone(),
                            },
                            emitter,
                        )
                        .await?;
                    self.append_plan_lifecycle_event(
                        &thread_id,
                        plan_id,
                        PlanLifecycleState::Accepted,
                        None,
                        reason.filter(|value| !value.trim().is_empty()),
                    )
                    .await?;
                    self.append_plan_lifecycle_event(
                        &thread_id,
                        plan_id,
                        PlanLifecycleState::Implementing,
                        None,
                        None,
                    )
                    .await?;
                    let prompt =
                        format!("{IMPLEMENT_PLAN_CURRENT_THREAD_PREFIX}\n\n{plan_content}");
                    let _ = self
                        .submit_prompt(StudioSubmitPromptRequest {
                            thread_id: thread_id.clone(),
                            prompt,
                            attachment_ids: Vec::new(),
                            options: StudioSubmitPromptOptions {
                                presentation: pl_core::MailboxPresentation::Hidden,
                                lifecycle: Some(StudioPlanImplementationLifecycle {
                                    thread_id: thread_id.clone(),
                                    plan_id: plan_id.clone(),
                                }),
                                ..StudioSubmitPromptOptions::default()
                            },
                        })
                        .await?;
                    Ok::<_, anyhow::Error>(resolved)
                }
                .await;
                match started {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        self.task_coordinator
                            .block_continuation_failure(
                                &run.id,
                                format!("plan implementation startup failed: {error}"),
                            )
                            .await?;
                        return Err(error);
                    }
                }
            }
            PlanConfirmationResolution::ContinuePlanning => {
                let adjustment = resolution_content
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .context("plan adjustment is empty")?;
                let continuation_resolution = InteractionResolution::PlanConfirmation {
                    decision: PlanConfirmationResolution::ContinuePlanning,
                    content: resolution_content.clone(),
                    reason: reason.clone(),
                };
                let message = format!(
                    "{CONTINUE_PLANNING_PREFIX}\n\n## 原计划\n\n{}\n\n## 用户调整要求\n\n{adjustment}",
                    content.trim()
                );
                let mail_id =
                    pl_core::AgentInteractionContinuationRequest::stable_mail_id(&interaction_id);
                let resolved = self
                    .submit_durable_interaction_continuation(
                        &current,
                        continuation_resolution,
                        message,
                        serde_json::json!({
                            "interactionResolutionId": interaction_id,
                            "interactionKind": "planConfirmation",
                            "originTurnId": current.scope.turn_id,
                            "planId": plan_id,
                            "mailId": mail_id,
                            "attachmentIds": [],
                        }),
                    )
                    .await?;
                self.append_plan_lifecycle_event(
                    &thread_id,
                    plan_id,
                    PlanLifecycleState::ContinuedPlanning,
                    None,
                    reason.or(resolution_content),
                )
                .await?;
                resolved
            }
            PlanConfirmationResolution::Dismiss => {
                let resolved = self
                    .interactions
                    .resolve(
                        &interaction_id,
                        InteractionResolution::PlanConfirmation {
                            decision: PlanConfirmationResolution::Dismiss,
                            content: resolution_content,
                            reason: reason.clone(),
                        },
                        emitter,
                    )
                    .await?;
                self.append_plan_lifecycle_event(
                    &thread_id,
                    plan_id,
                    PlanLifecycleState::Dismissed,
                    None,
                    reason,
                )
                .await?;
                resolved
            }
        };

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

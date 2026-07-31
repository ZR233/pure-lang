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

const IMPLEMENT_PLAN_CURRENT_SESSION_PREFIX: &str = "A previous agent produced the plan below to accomplish the user's task. Implement the plan in the current session. Treat the plan as the source of user intent, re-read files as needed, and carry the work through implementation and verification.";

impl StudioRuntime {
    pub(super) async fn resolve_plan_confirmation(
        &self,
        interaction_id: String,
        current: InteractionRequest,
        resolution: InteractionResolution,
        emitter: InteractionEmitter,
    ) -> Result<StudioResolveInteractionResponse> {
        let session_id = current.scope.session_id.clone();
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
                session_id,
                interaction: current,
                sessions: Vec::new(),
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
                self.task_coordinator
                    .validate_confirmed_plan_design_contract(&plan_content)?;
                let session = self
                    .store
                    .read_session(&session_id)
                    .await?
                    .context("task session not found")?;
                if session.mode != StudioMode::Task.label() {
                    bail!("plan implementation requires a Task mode session");
                }
                let project = self
                    .store
                    .read_project(&session.project_id)
                    .await?
                    .context("task project not found")?;
                let run = self
                    .task_coordinator
                    .start_confirmed_task(&session_id, &plan_content, &project.path)
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
                        &session_id,
                        plan_id,
                        PlanLifecycleState::Accepted,
                        None,
                        reason.filter(|value| !value.trim().is_empty()),
                    )
                    .await?;
                    self.append_plan_lifecycle_event(
                        &session_id,
                        plan_id,
                        PlanLifecycleState::Implementing,
                        None,
                        None,
                    )
                    .await?;
                    let prompt =
                        format!("{IMPLEMENT_PLAN_CURRENT_SESSION_PREFIX}\n\n{plan_content}");
                    let _ = self
                        .submit_prompt(StudioSubmitPromptRequest {
                            session_id: session_id.clone(),
                            prompt,
                            attachment_ids: Vec::new(),
                            options: StudioSubmitPromptOptions {
                                presentation: pl_core::MailboxPresentation::SyntheticHidden,
                                lifecycle: Some(StudioPlanImplementationLifecycle {
                                    session_id: session_id.clone(),
                                    plan_id: plan_id.clone(),
                                }),
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
                let resolved = self
                    .interactions
                    .resolve(
                        &interaction_id,
                        InteractionResolution::PlanConfirmation {
                            decision: PlanConfirmationResolution::ContinuePlanning,
                            content: resolution_content.clone(),
                            reason: reason.clone(),
                        },
                        emitter,
                    )
                    .await?;
                self.append_plan_lifecycle_event(
                    &session_id,
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
                    &session_id,
                    plan_id,
                    PlanLifecycleState::Dismissed,
                    None,
                    reason,
                )
                .await?;
                resolved
            }
        };

        let sessions = if let Some(session) = self.store.read_session(&session_id).await? {
            self.store.list_sessions(&session.project_id).await?
        } else {
            Vec::new()
        };

        Ok(StudioResolveInteractionResponse {
            session_id,
            interaction: resolved,
            sessions,
        })
    }
}

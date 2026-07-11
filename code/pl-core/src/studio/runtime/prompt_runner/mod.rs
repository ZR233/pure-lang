use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_protocol::{
    ContentPart, ImageSource, InteractionChangedEvent, InteractionKind, InteractionResolution,
    MessageContent, PlanLifecycleEvent, PlanLifecycleState, StudioEventKind, StudioMessage,
    StudioMessageRole, StudioMessageStatus, StudioPart, StudioPartStatus, StudioPartType,
    StudioTextChannel, StudioTurnStatus,
};
use pl_trace::{TraceEvent, TraceEventKind, TracePart, TracePartKind};
use tokio_util::sync::CancellationToken;

use crate::config::ModelRole;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::records::StudioPromptOutcome;
use crate::studio::{InteractionEmitter, resolution_matches_kind};
use crate::{
    CompileMode, InstructionAssembler, InstructionAssemblyRequest, InstructionSnapshot, PureCore,
    TraceRecorder, TurnAbortReason, TurnOptions, TurnRequest, TurnResultStatus,
    load_workspace_instructions, resolve_workspace_root,
};

use super::self_learning::{should_start_self_learning, spawn_self_learning_review};
use super::{
    PromptHistoryPolicy, RunPromptRequest, StudioResolveInteractionResponse, StudioRuntime,
    StudioStopPromptResponse, StudioSubmitPromptOptions, StudioSubmitPromptRequest,
    StudioSubmitPromptResponse,
};

mod inner;

impl StudioRuntime {
    pub async fn submit_prompt(
        &self,
        request: StudioSubmitPromptRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        let StudioSubmitPromptRequest {
            session_id,
            prompt,
            attachment_ids,
            options,
        } = request;
        if prompt.trim().is_empty() && attachment_ids.is_empty() {
            bail!("prompt is empty");
        }
        let turn_id = new_id("turn");
        let cancellation_token = CancellationToken::new();
        self.active_turns
            .insert(
                session_id.clone(),
                turn_id.clone(),
                cancellation_token.clone(),
            )
            .await?;
        let submit_result = async {
            self.events
                .emit_turn(&session_id, &turn_id, StudioTurnStatus::Queued, None)
                .await?;
            self.events
                .emit_turn(
                    &session_id,
                    &turn_id,
                    StudioTurnStatus::ContextLoading,
                    None,
                )
                .await?;
            self.emit_user_prompt_snapshots(
                &session_id,
                &turn_id,
                &prompt,
                &attachment_ids,
                &options,
            )
            .await?;
            self.store.next_studio_event_sequence(&session_id).await
        }
        .await;
        let cursor = match submit_result {
            Ok(cursor) => cursor as u64,
            Err(error) => {
                self.active_turn_removed(&session_id).await;
                return Err(error);
            }
        };
        let run_runtime = self.clone();
        let run_session_id = session_id.clone();
        let run_turn_id = turn_id.clone();
        tokio::spawn(async move {
            run_runtime
                .run_prompt_background(
                    run_session_id,
                    run_turn_id,
                    prompt,
                    attachment_ids,
                    cancellation_token,
                    options,
                )
                .await;
        });
        Ok(StudioSubmitPromptResponse {
            session_id,
            turn_id,
            cursor,
        })
    }

    pub async fn stop_prompt(&self, session_id: String) -> Result<StudioStopPromptResponse> {
        let token = self.active_turns.token(&session_id).await;
        let Some(token) = token else {
            return Ok(StudioStopPromptResponse {
                session_id,
                stopped: false,
            });
        };
        token.cancel();
        let emitter = self.interaction_emitter(session_id.clone());
        self.interactions
            .cancel_session(&session_id, "interrupted by user", emitter)
            .await?;
        Ok(StudioStopPromptResponse {
            session_id,
            stopped: true,
        })
    }

    pub async fn resolve_interaction(
        &self,
        interaction_id: String,
        resolution: InteractionResolution,
    ) -> Result<StudioResolveInteractionResponse> {
        let current = self
            .store
            .read_interaction(&interaction_id)
            .await?
            .context("interaction not found")?;
        let session_id = current.scope.session_id.clone();
        if !resolution_matches_kind(&current.kind, &resolution) {
            bail!("interaction resolution kind does not match interaction");
        }
        let emitter = self.interaction_emitter(session_id.clone());

        if current.kind == InteractionKind::PlanConfirmation {
            return self
                .resolve_plan_confirmation(interaction_id, current, resolution, emitter)
                .await;
        }

        let resolved = self
            .interactions
            .resolve(&interaction_id, resolution, emitter)
            .await?;
        Ok(StudioResolveInteractionResponse {
            session_id,
            interaction: resolved,
            sessions: Vec::new(),
        })
    }

    pub async fn run_prompt(&self, request: RunPromptRequest) -> Result<StudioPromptOutcome> {
        let mut request = request;
        let session_id = request.session_id.clone();
        let turn_id = request.turn_id.clone();
        let cancellation_token = request
            .options
            .cancellation_token
            .clone()
            .unwrap_or_default();
        if request.options.cancellation_token.is_none() {
            request.options = request
                .options
                .with_cancellation(cancellation_token.clone());
        }
        self.active_turns
            .insert(session_id.clone(), turn_id, cancellation_token)
            .await?;
        let outcome = self
            .run_prompt_inner(request, PromptHistoryPolicy::Persist)
            .await;
        self.active_turn_removed(&session_id).await;
        outcome
    }

    async fn run_prompt_background(
        &self,
        session_id: String,
        turn_id: String,
        prompt: String,
        attachment_ids: Vec<String>,
        cancellation_token: CancellationToken,
        submit_options: StudioSubmitPromptOptions,
    ) {
        let history_policy = submit_options.history_policy;
        let lifecycle = submit_options.lifecycle;
        let _ = self
            .events
            .emit_turn(
                &session_id,
                &turn_id,
                StudioTurnStatus::WaitingForModel,
                None,
            )
            .await;
        let emitter = self.interaction_emitter(session_id.clone());
        let interaction_callback = self
            .interactions
            .callback(session_id.clone(), emitter.clone());
        let options = TurnOptions::default()
            .with_cancellation(cancellation_token)
            .with_interaction_callback(interaction_callback.clone());
        let result = self
            .run_prompt_inner(
                RunPromptRequest {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    prompt,
                    attachment_ids,
                    interaction_callback,
                    interaction_emitter: emitter.clone(),
                    options,
                },
                history_policy,
            )
            .await;
        self.active_turn_removed(&session_id).await;
        let _ = self
            .interactions
            .cancel_transient_interactions(&session_id, "turn completed", emitter)
            .await;
        match result {
            Ok(outcome) => {
                self.emit_turn_completion(&session_id, &turn_id, &outcome)
                    .await;
                if let Some(lifecycle) = lifecycle {
                    let (state, reason) = match outcome.result.status {
                        TurnResultStatus::Completed => (PlanLifecycleState::Implemented, None),
                        TurnResultStatus::Aborted => (
                            PlanLifecycleState::ImplementationFailed,
                            outcome
                                .result
                                .abort_reason
                                .map(|reason| reason.as_str().to_string())
                                .or_else(|| Some("turn aborted".to_string())),
                        ),
                        TurnResultStatus::Errored => (
                            PlanLifecycleState::ImplementationFailed,
                            outcome
                                .result
                                .error
                                .or_else(|| Some("turn errored".to_string())),
                        ),
                    };
                    let _ = self
                        .append_plan_lifecycle_event(
                            &lifecycle.session_id,
                            &lifecycle.plan_id,
                            state,
                            Some(turn_id),
                            reason,
                        )
                        .await;
                }
            }
            Err(error) => {
                let _ = self
                    .events
                    .emit_turn(
                        &session_id,
                        &turn_id,
                        StudioTurnStatus::Failed,
                        Some(format!("{error:#}")),
                    )
                    .await;
            }
        }
    }

    async fn emit_user_prompt_snapshots(
        &self,
        session_id: &str,
        turn_id: &str,
        prompt: &str,
        attachment_ids: &[String],
        options: &StudioSubmitPromptOptions,
    ) -> Result<()> {
        let now = unix_seconds();
        let message_id = format!("{turn_id}:user");
        let part_id = format!("{turn_id}:user-text");
        let attachments = self
            .store
            .load_attachments(session_id, attachment_ids)
            .await?
            .iter()
            .map(crate::studio::studio_attachment)
            .collect::<Vec<_>>();
        let message = StudioMessage {
            message_id: message_id.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            role: StudioMessageRole::User,
            status: StudioMessageStatus::Completed,
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
            error: None,
            metadata: if options.user_prompt.is_synthetic() || options.user_prompt.is_ignored() {
                serde_json::json!({
                    "synthetic": options.user_prompt.is_synthetic(),
                    "ignored": options.user_prompt.is_ignored(),
                })
            } else {
                serde_json::json!({})
            },
        };
        self.events
            .emit(
                None,
                Some(session_id.to_string()),
                Some(turn_id.to_string()),
                StudioEventKind::MessageUpdated {
                    message: Box::new(message),
                },
            )
            .await?;
        let part = StudioPart {
            part_id,
            message_id,
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            part_type: StudioPartType::Text,
            order: 0,
            revision: 0,
            status: StudioPartStatus::Completed,
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
            error: None,
            text_channel: Some(StudioTextChannel::User),
            activity_group_id: None,
            text: options.user_prompt.visible_prompt(prompt).to_string(),
            attachments,
            tool: None,
            agent: None,
            inference: None,
            plan: None,
            file: None,
            usage: None,
            synthetic: options.user_prompt.is_synthetic(),
            ignored: options.user_prompt.is_ignored(),
        };
        self.events
            .emit(
                None,
                Some(session_id.to_string()),
                Some(turn_id.to_string()),
                StudioEventKind::MessagePartUpdated {
                    part: Box::new(part),
                },
            )
            .await?;
        Ok(())
    }

    async fn emit_turn_completion(
        &self,
        session_id: &str,
        turn_id: &str,
        outcome: &StudioPromptOutcome,
    ) {
        let status = match outcome.result.status {
            TurnResultStatus::Completed => StudioTurnStatus::Completed,
            TurnResultStatus::Aborted
                if outcome.result.abort_reason == Some(TurnAbortReason::Interrupted) =>
            {
                StudioTurnStatus::Cancelled
            }
            TurnResultStatus::Aborted | TurnResultStatus::Errored => StudioTurnStatus::Failed,
        };
        let reason = outcome.result.error.clone().or_else(|| {
            outcome
                .result
                .abort_reason
                .map(|reason| reason.as_str().to_string())
        });
        let _ = self
            .events
            .emit_turn(session_id, turn_id, status, reason)
            .await;
    }

    pub(super) async fn append_plan_lifecycle_event(
        &self,
        session_id: &str,
        plan_id: &str,
        state: PlanLifecycleState,
        turn_id: Option<String>,
        reason: Option<String>,
    ) -> Result<()> {
        self.events
            .emit(
                None,
                Some(session_id.to_string()),
                turn_id.clone(),
                StudioEventKind::PlanLifecycleChanged {
                    event: PlanLifecycleEvent {
                        plan_id: plan_id.to_string(),
                        state,
                        turn_id,
                        reason,
                        updated_at: unix_seconds(),
                    },
                },
            )
            .await?;
        Ok(())
    }

    pub(super) fn interaction_emitter(&self, session_id: String) -> InteractionEmitter {
        let runtime = self.clone();
        Arc::new(move |interaction| {
            let runtime = runtime.clone();
            let session_id = session_id.clone();
            Box::pin(async move {
                runtime
                    .events
                    .emit_interaction(&session_id, InteractionChangedEvent { interaction })
                    .await?;
                Ok(())
            })
        })
    }

    pub(super) async fn cancel_recovered_transient_interactions(
        &self,
        cancelled_turns: Vec<crate::studio::records::StudioTurnRecord>,
    ) -> Result<()> {
        let mut session_ids = cancelled_turns
            .into_iter()
            .map(|turn| turn.session_id)
            .collect::<Vec<_>>();
        session_ids.extend(
            self.store
                .list_sessions_with_transient_pending_interactions()
                .await?,
        );
        session_ids.sort();
        session_ids.dedup();
        for session_id in session_ids {
            let emitter = self.interaction_emitter(session_id.clone());
            self.interactions
                .cancel_transient_interactions(&session_id, "application restarted", emitter)
                .await?;
        }
        Ok(())
    }
}

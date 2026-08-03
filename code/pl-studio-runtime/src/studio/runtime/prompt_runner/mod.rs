use std::sync::Arc;

use crate::{
    InteractionChangedEvent, InteractionKind, InteractionResolution, InteractionStatus,
    PlanLifecycleEvent, PlanLifecycleState,
};
use anyhow::{Context, Result, bail};

use crate::StudioMode;
use crate::config::StudioRole;
use crate::studio::agent_host::root_agent_id;
use crate::studio::ids::unix_seconds;
use crate::studio::task_coordinator::{TaskStopOrigin, TaskStopReason};
use crate::studio::{InteractionEmitter, resolution_matches_kind};
use crate::studio::{SessionKind, SessionRecord};

use super::{
    StudioResolveInteractionResponse, StudioRuntime, StudioStopPromptResponse,
    StudioSubmitPromptOptions, StudioSubmitPromptRequest, StudioSubmitPromptResponse,
};

const TASK_RESUME_MESSAGE: &str = "Resume the active Task from canonical durable state. Read task_status and list_agents before choosing the next state-machine transition. Do not recreate completed work or infer review outcomes from pre-restart context.";

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
        // Serialize turn registration with the updater's final idle check.
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        if !matches!(
            self.runtime_snapshot().status,
            crate::StudioRuntimeStatus::Ready
        ) {
            bail!("Studio runtime is not ready");
        }
        let (handle, agent_id) = self.ensure_session_agent(&session_id).await?;
        let session = pl_core::SessionId::new(session_id.clone())?;
        let metadata = submit_metadata(&attachment_ids, &options);
        let presentation = options.presentation.clone();
        let turn_id = handle
            .submit(
                agent_id.clone(),
                pl_core::AgentSubmitRequest::start(session.clone(), prompt.clone())
                    .with_presentation(presentation)
                    .with_metadata(metadata),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let cursor = handle
            .session_snapshot(&session)
            .map_err(|error| anyhow::anyhow!(error))?
            .through_sequence;
        Ok(StudioSubmitPromptResponse {
            session_id,
            turn_id: turn_id.into_string(),
            cursor,
        })
    }

    /// Resumes a paused Task after an explicit user action without projecting a
    /// synthetic user message into the Planner timeline.
    pub async fn resume_task(&self, session_id: String) -> Result<StudioSubmitPromptResponse> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        if !matches!(
            self.runtime_snapshot().status,
            crate::StudioRuntimeStatus::Ready
        ) {
            bail!("Studio runtime is not ready");
        }
        let session_record = self
            .store
            .read_session(&session_id)
            .await?
            .context("resume Task session not found")?;
        if session_record.session_kind != SessionKind::Root {
            bail!("only a root Task session can be resumed");
        }
        if session_record.agent_status != "interrupted" {
            bail!("Task session is not paused");
        }
        let run = self
            .store
            .find_active_task_run_for_session(&session_id)
            .await?
            .context("active Task run not found")?;
        let (handle, agent_id) = self.ensure_session_agent(&session_id).await?;
        let snapshot = handle
            .snapshot(agent_id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        if snapshot.active_turn_id.is_some()
            || snapshot.pending_inputs > 0
            || snapshot.activity != pl_core::AgentActivityState::Idle
        {
            bail!("Task Planner is already active or has pending input");
        }

        let session = pl_core::SessionId::new(session_id.clone())?;
        let resume_revision = session_record
            .agent_updated_at
            .unwrap_or(session_record.updated_at);
        let mail_id = format!("task-resume:{}:{resume_revision}", run.id);
        let metadata = serde_json::json!({
            "kind": "taskResume",
            "taskRunId": run.id,
            "source": "user",
        });
        let turn_id = handle
            .submit(
                agent_id,
                pl_core::AgentSubmitRequest::start(session.clone(), TASK_RESUME_MESSAGE)
                    .with_presentation(pl_core::MailboxPresentation::Hidden)
                    .with_metadata(metadata)
                    .with_mail_id(mail_id),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let cursor = handle
            .session_snapshot(&session)
            .map_err(|error| anyhow::anyhow!(error))?
            .through_sequence;
        Ok(StudioSubmitPromptResponse {
            session_id,
            turn_id: turn_id.into_string(),
            cursor,
        })
    }

    pub async fn stop_prompt(&self, session_id: String) -> Result<StudioStopPromptResponse> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        if self
            .store
            .find_active_task_run_for_session(&session_id)
            .await?
            .is_some()
        {
            let reason = TaskStopReason::new("用户在 Studio 中请求停止任务")
                .expect("fixed user stop reason must not be empty");
            self.task_coordinator
                .stop_task(&session_id, &handle, TaskStopOrigin::UserRequest, reason)
                .await?;
            let emitter = self.interaction_emitter(session_id.clone());
            self.interactions
                .cancel_session(&session_id, "interrupted by user", emitter)
                .await?;
            return Ok(StudioStopPromptResponse {
                session_id,
                stopped: true,
            });
        }
        let agent_id = self.session_owner_agent_id(&session_id).await?;
        let snapshot = match handle.snapshot(agent_id.clone()).await {
            Ok(snapshot) => snapshot,
            Err(pl_core::AgentRuntimeError::NotFound(_)) => {
                return Ok(StudioStopPromptResponse {
                    session_id,
                    stopped: false,
                });
            }
            Err(error) => return Err(anyhow::anyhow!(error)),
        };
        let Some(turn_id) = snapshot.active_turn_id else {
            return Ok(StudioStopPromptResponse {
                session_id,
                stopped: false,
            });
        };
        match handle.cancel_turn(agent_id, turn_id).await {
            Ok(()) => {}
            Err(pl_core::AgentRuntimeError::NoActiveTurn(_))
            | Err(pl_core::AgentRuntimeError::TurnMismatch { .. }) => {
                return Ok(StudioStopPromptResponse {
                    session_id,
                    stopped: false,
                });
            }
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
        let emitter = self.interaction_emitter(session_id.clone());
        self.interactions
            .cancel_session(&session_id, "interrupted by user", emitter)
            .await?;
        Ok(StudioStopPromptResponse {
            session_id,
            stopped: true,
        })
    }

    async fn ensure_session_agent(
        &self,
        studio_session_id: &str,
    ) -> Result<(pl_core::AgentRuntimeHandle, pl_core::AgentId)> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        let target = self.read_owned_session(studio_session_id).await?;
        let target_agent_id = pl_core::AgentId::new(target.owner_agent_id.clone())?;
        let mut missing = Vec::new();
        let mut current = target;
        loop {
            let owner_agent_id = pl_core::AgentId::new(current.owner_agent_id.clone())?;
            match handle.snapshot(owner_agent_id).await {
                Ok(_) => break,
                Err(pl_core::AgentRuntimeError::NotFound(_)) => {}
                Err(error) => return Err(anyhow::anyhow!(error)),
            }
            let parent_session_id = current.parent_session_id.clone();
            missing.push(current);
            let Some(parent_session_id) = parent_session_id else {
                break;
            };
            current = self.read_owned_session(&parent_session_id).await?;
        }

        for session_record in missing.into_iter().rev() {
            let registration = self
                .session_agent_registration(&handle, session_record)
                .await?;
            match handle.register(registration).await {
                Ok(_) | Err(pl_core::AgentRuntimeError::AlreadyExists(_)) => {}
                Err(error) => return Err(anyhow::anyhow!(error)),
            }
        }
        handle
            .snapshot(target_agent_id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok((handle, target_agent_id))
    }

    async fn session_agent_registration(
        &self,
        handle: &pl_core::AgentRuntimeHandle,
        session_record: SessionRecord,
    ) -> Result<pl_core::AgentRegistration> {
        let agent_id = pl_core::AgentId::new(session_record.owner_agent_id.clone())?;
        let (parent_id, role, depth) = match session_record.session_kind {
            SessionKind::Root => {
                anyhow::ensure!(
                    session_record.parent_session_id.is_none()
                        && agent_id == root_agent_id(&session_record.id),
                    "root Studio session {} has invalid canonical owner",
                    session_record.id
                );
                let role = match StudioMode::from_label(&session_record.mode) {
                    StudioMode::Simple => StudioRole::Executor,
                    StudioMode::Task => StudioRole::Planner,
                };
                (None, role, 0)
            }
            SessionKind::Agent => {
                anyhow::ensure!(
                    agent_id != root_agent_id(&session_record.id),
                    "child Studio session {} cannot use a root agent identity",
                    session_record.id
                );
                let parent_session_id = session_record
                    .parent_session_id
                    .as_deref()
                    .context("child Studio session has no parent session")?;
                let parent = self.read_owned_session(parent_session_id).await?;
                let parent_id = pl_core::AgentId::new(parent.owner_agent_id)?;
                let parent_snapshot = handle
                    .snapshot(parent_id.clone())
                    .await
                    .map_err(|error| anyhow::anyhow!(error))?;
                let role = StudioRole::from_key(&session_record.owner_role)
                    .context("child Studio session has an unsupported owner role")?;
                (Some(parent_id), role, parent_snapshot.identity.depth + 1)
            }
        };
        let session_id = pl_core::SessionId::new(session_record.id.clone())?;
        let registration = pl_core::AgentRegistration {
            identity: pl_core::AgentIdentity {
                id: agent_id,
                parent_id,
                role: role.id(),
                depth,
            },
            session: pl_core::AgentSessionState {
                id: session_id,
                metadata: serde_json::json!({
                    "projectId": session_record.project_id,
                    "title": session_record.title,
                }),
                session: pl_core::AgentSession::new(),
                usage: pl_model::TokenUsage::default(),
                last_context_tokens: None,
                trace_sequence: 0,
                session_event_sequence: 0,
            },
        };
        Ok(registration)
    }

    async fn read_owned_session(&self, session_id: &str) -> Result<SessionRecord> {
        self.store
            .read_session(session_id)
            .await?
            .context("selected session not found")
    }

    async fn session_owner_agent_id(&self, session_id: &str) -> Result<pl_core::AgentId> {
        let session = self.read_owned_session(session_id).await?;
        pl_core::AgentId::new(session.owner_agent_id).map_err(Into::into)
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

        if current.status != InteractionStatus::Pending {
            return Ok(StudioResolveInteractionResponse {
                session_id,
                interaction: current,
                sessions: Vec::new(),
            });
        }
        let detached_user_input_owner = if current.kind == InteractionKind::UserInput {
            let (handle, owner_agent_id) = self.ensure_session_agent(&session_id).await?;
            let snapshot = handle
                .snapshot(owner_agent_id.clone())
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
            let origin_turn_is_active = snapshot
                .active_turn_id
                .as_ref()
                .is_some_and(|turn_id| turn_id.as_str() == current.scope.turn_id.as_str());
            (!origin_turn_is_active).then_some(owner_agent_id)
        } else {
            None
        };
        let resolved = if let Some(owner_agent_id) = detached_user_input_owner {
            let (handle, canonical_owner) = self.ensure_session_agent(&session_id).await?;
            anyhow::ensure!(
                canonical_owner == owner_agent_id,
                "interaction answer resolved to a different canonical owner"
            );
            let mail_id = format!("interaction-resolution:{interaction_id}");
            let message = serde_json::to_string_pretty(&serde_json::json!({
                "type": "studioInteractionResolution",
                "interactionId": interaction_id,
                "originTurnId": current.scope.turn_id,
                "payload": current.payload,
                "resolution": resolution,
            }))?;
            handle
                .submit_current_session(
                    canonical_owner,
                    pl_core::AgentCurrentSessionSubmitRequest::start(message)
                        .with_presentation(pl_core::MailboxPresentation::Hidden)
                        .with_mail_id(mail_id.clone())
                        .with_metadata(serde_json::json!({
                            "interactionResolutionId": interaction_id,
                            "mailId": mail_id,
                            "attachmentIds": [],
                        })),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
            self.interactions
                .resolve(&interaction_id, resolution, emitter)
                .await?
        } else {
            self.interactions
                .resolve(&interaction_id, resolution, emitter)
                .await?
        };
        Ok(StudioResolveInteractionResponse {
            session_id,
            interaction: resolved,
            sessions: Vec::new(),
        })
    }

    pub(super) async fn append_plan_lifecycle_event(
        &self,
        session_id: &str,
        plan_id: &str,
        state: PlanLifecycleState,
        turn_id: Option<String>,
        reason: Option<String>,
    ) -> Result<()> {
        let updated_at = unix_seconds();
        self.record_session_facts(
            session_id,
            vec![pl_core::SessionEventFact::durable(
                None,
                turn_id.clone(),
                updated_at,
                crate::SessionEventKind::PlanChanged {
                    event: PlanLifecycleEvent {
                        plan_id: plan_id.to_string(),
                        state,
                        turn_id,
                        reason,
                        updated_at,
                    },
                },
            )],
        )
        .await?;
        Ok(())
    }

    pub(super) async fn record_session_facts(
        &self,
        session_id: &str,
        facts: Vec<pl_core::SessionEventFact>,
    ) -> Result<()> {
        let runtime = self.agent_framework().await?.handle();
        let owner_agent_id = self.session_owner_agent_id(session_id).await?;
        runtime
            .record_session_facts(
                owner_agent_id,
                pl_core::SessionId::new(session_id.to_string())?,
                facts,
            )
            .await
            .map_err(Into::into)
    }

    pub(super) fn interaction_emitter(&self, session_id: String) -> InteractionEmitter {
        let runtime = self.clone();
        Arc::new(move |interaction| {
            let runtime = runtime.clone();
            let session_id = session_id.clone();
            Box::pin(async move {
                runtime
                    .record_session_facts(
                        &session_id,
                        vec![pl_core::SessionEventFact::durable(
                            None,
                            Some(interaction.scope.turn_id.clone()),
                            interaction.updated_at,
                            crate::SessionEventKind::InteractionChanged {
                                event: Box::new(InteractionChangedEvent { interaction }),
                            },
                        )],
                    )
                    .await?;
                Ok(())
            })
        })
    }

    pub(super) async fn recover_interactions_after_restart(&self) -> Result<()> {
        for interaction in self.store.list_restart_recoverable_user_inputs().await? {
            let session_id = interaction.scope.session_id.clone();
            let _ = self.ensure_session_agent(&session_id).await?;
            let emitter = self.interaction_emitter(session_id);
            let recovered = self
                .interactions
                .recover_user_input(interaction, emitter)
                .await?;
            self.store
                .mark_restart_user_input_recovered(&recovered)
                .await?;
        }

        let mut session_ids = self
            .store
            .list_sessions_with_transient_pending_interactions()
            .await?;
        session_ids.sort();
        session_ids.dedup();
        for session_id in session_ids {
            // pending interaction 可能先于 framework registration 持久化。
            // 先恢复 canonical owner，事件仍由 PL actor 分配序列、持久化并广播。
            let _ = self.ensure_session_agent(&session_id).await?;
            let emitter = self.interaction_emitter(session_id.clone());
            self.interactions
                .cancel_recovered_tool_approvals(
                    &session_id,
                    "application restarted before approval completed",
                    emitter,
                )
                .await?;
        }
        Ok(())
    }
}

fn submit_metadata(
    attachment_ids: &[String],
    options: &StudioSubmitPromptOptions,
) -> serde_json::Value {
    let lifecycle = options.lifecycle.as_ref().map(|lifecycle| {
        serde_json::json!({
            "sessionId": lifecycle.session_id,
            "planId": lifecycle.plan_id,
        })
    });
    serde_json::json!({
        "attachmentIds": attachment_ids,
        "historyPolicy": "persist",
        "planLifecycle": lifecycle,
    })
}

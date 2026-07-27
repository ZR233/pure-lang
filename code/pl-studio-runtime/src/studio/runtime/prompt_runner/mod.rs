use std::sync::Arc;

use crate::{
    InteractionChangedEvent, InteractionKind, InteractionResolution, PlanLifecycleEvent,
    PlanLifecycleState,
};
use anyhow::{Context, Result, bail};

use crate::StudioMode;
use crate::config::StudioRole;
use crate::studio::agent_host::root_agent_id;
use crate::studio::ids::unix_seconds;
use crate::studio::{InteractionEmitter, resolution_matches_kind};

use super::{
    StudioResolveInteractionResponse, StudioRuntime, StudioStopPromptResponse,
    StudioSubmitPromptOptions, StudioSubmitPromptRequest, StudioSubmitPromptResponse,
};

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
        let (handle, agent_id) = self.ensure_root_agent(&session_id).await?;
        let session = pl_core::SessionId::new(session_id.clone())?;
        let metadata = submit_metadata(&attachment_ids, &options);
        let turn_id = handle
            .submit(
                agent_id.clone(),
                pl_core::AgentSubmitRequest::start(session.clone(), prompt.clone())
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

    pub async fn stop_prompt(&self, session_id: String) -> Result<StudioStopPromptResponse> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        let agent_id = root_agent_id(&session_id);
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

    async fn ensure_root_agent(
        &self,
        studio_session_id: &str,
    ) -> Result<(pl_core::AgentRuntimeHandle, pl_core::AgentId)> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        let agent_id = root_agent_id(studio_session_id);
        match handle.snapshot(agent_id.clone()).await {
            Ok(_) => return Ok((handle, agent_id)),
            Err(pl_core::AgentRuntimeError::NotFound(_)) => {}
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
        let session_record = self
            .store
            .read_session(studio_session_id)
            .await?
            .context("selected session not found")?;
        let role = match StudioMode::from_label(&session_record.mode) {
            StudioMode::Simple => StudioRole::Executor,
            StudioMode::Task => StudioRole::Planner,
        };
        let session_id = pl_core::SessionId::new(studio_session_id.to_string())?;
        let registration = pl_core::AgentRegistration {
            identity: pl_core::AgentIdentity {
                id: agent_id.clone(),
                parent_id: None,
                role: role.id(),
                depth: 0,
            },
            wake_policy: pl_core::AgentWakePolicy::RuntimeTerminal,
            sessions: vec![pl_core::AgentSessionState {
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
            }],
        };
        match handle.register(registration).await {
            Ok(_) | Err(pl_core::AgentRuntimeError::AlreadyExists(_)) => {}
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
        Ok((handle, agent_id))
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
        runtime
            .record_session_facts(
                root_agent_id(session_id),
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

    pub(super) async fn cancel_recovered_transient_interactions(&self) -> Result<()> {
        let mut session_ids = self
            .store
            .list_sessions_with_transient_pending_interactions()
            .await?;
        session_ids.sort();
        session_ids.dedup();
        for session_id in session_ids {
            // 旧数据库可能有 pending interaction，但尚未写入 framework registration。
            // 先注册 owner，恢复事件仍由 PL actor 分配序列、持久化并广播。
            let _ = self.ensure_root_agent(&session_id).await?;
            let emitter = self.interaction_emitter(session_id.clone());
            self.interactions
                .cancel_transient_interactions(&session_id, "application restarted", emitter)
                .await?;
        }
        Ok(())
    }
}

fn submit_metadata(
    attachment_ids: &[String],
    options: &StudioSubmitPromptOptions,
) -> serde_json::Value {
    let (visible_prompt, synthetic, ignored) = match &options.user_prompt {
        super::StudioUserPromptPresentation::Normal => (None, false, false),
        super::StudioUserPromptPresentation::SyntheticVisible { visible_prompt } => {
            (Some(visible_prompt.clone()), true, false)
        }
        super::StudioUserPromptPresentation::SyntheticIgnored { visible_prompt } => {
            (Some(visible_prompt.clone()), true, true)
        }
    };
    let lifecycle = options.lifecycle.as_ref().map(|lifecycle| {
        serde_json::json!({
            "sessionId": lifecycle.session_id,
            "planId": lifecycle.plan_id,
        })
    });
    serde_json::json!({
        "attachmentIds": attachment_ids,
        "userPrompt": {
            "visiblePrompt": visible_prompt,
            "synthetic": synthetic,
            "ignored": ignored,
        },
        "historyPolicy": "persist",
        "planLifecycle": lifecycle,
    })
}

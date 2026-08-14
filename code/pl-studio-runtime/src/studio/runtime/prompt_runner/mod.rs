use std::sync::Arc;

use crate::{InteractionKind, InteractionResolution, InteractionStatus, PlanLifecycleState};
use anyhow::{Context, Result, bail};
use futures::FutureExt;

use crate::StudioMode;
use crate::config::StudioRole;
use crate::studio::agent_host::root_agent_id;
use crate::studio::task_coordinator::{TaskStopOrigin, TaskStopReason};
use crate::studio::{InteractionEmitter, resolution_matches_kind};
use crate::studio::{ThreadKind, ThreadRecord};

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
            thread_id,
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
            self.runtime_snapshot().await?.status,
            crate::StudioRuntimeStatus::Ready
        ) {
            bail!("Studio runtime is not ready");
        }
        let (handle, agent_id) = self.ensure_thread_agent(&thread_id).await?;
        let thread = pl_core::ThreadId::new(thread_id.clone())?;
        let metadata = submit_metadata(&attachment_ids, &options);
        let presentation = options.presentation.clone();
        let turn_id = handle
            .submit(
                agent_id.clone(),
                pl_core::AgentSubmitRequest::start(thread.clone(), prompt.clone())
                    .with_presentation(presentation)
                    .with_metadata(metadata)
                    .with_turn_policy(options.turn_policy),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let cursor = handle
            .thread_snapshot(&thread)
            .map_err(|error| anyhow::anyhow!(error))?
            .revision;
        Ok(StudioSubmitPromptResponse {
            thread_id,
            turn_id: turn_id.into_string(),
            cursor,
        })
    }

    /// Resumes a paused Task after an explicit user action without projecting a
    /// synthetic user message into the Planner timeline.
    pub async fn resume_task(&self, thread_id: String) -> Result<StudioSubmitPromptResponse> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        if !matches!(
            self.runtime_snapshot().await?.status,
            crate::StudioRuntimeStatus::Ready
        ) {
            bail!("Studio runtime is not ready");
        }
        let thread_record = self
            .store
            .read_thread(&thread_id)
            .await?
            .context("resume Task Thread not found")?;
        if thread_record.thread_kind != ThreadKind::Root {
            bail!("only a root Task Thread can be resumed");
        }
        if thread_record.status != "idle" {
            bail!("Task Thread is not paused");
        }
        let run = self
            .store
            .find_active_task_run_for_root_thread(&thread_id)
            .await?
            .context("active Task run not found")?;
        let (handle, agent_id) = self.ensure_thread_agent(&thread_id).await?;
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

        let thread = pl_core::ThreadId::new(thread_id.clone())?;
        let resume_revision = thread_record
            .runtime_updated_at
            .unwrap_or(thread_record.updated_at);
        let mail_id = format!("task-resume:{}:{resume_revision}", run.id);
        let metadata = serde_json::json!({
            "kind": "taskResume",
            "taskRunId": run.id,
            "source": "user",
        });
        let turn_id = handle
            .submit(
                agent_id,
                pl_core::AgentSubmitRequest::start(thread.clone(), TASK_RESUME_MESSAGE)
                    .with_presentation(pl_core::MailboxPresentation::Hidden)
                    .with_metadata(metadata)
                    .with_mail_id(mail_id),
            )
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let cursor = handle
            .thread_snapshot(&thread)
            .map_err(|error| anyhow::anyhow!(error))?
            .revision;
        Ok(StudioSubmitPromptResponse {
            thread_id,
            turn_id: turn_id.into_string(),
            cursor,
        })
    }

    pub async fn stop_prompt(&self, thread_id: String) -> Result<StudioStopPromptResponse> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        if self
            .store
            .find_active_task_run_for_root_thread(&thread_id)
            .await?
            .is_some()
        {
            let reason = TaskStopReason::new("用户在 Studio 中请求停止任务")
                .expect("fixed user stop reason must not be empty");
            self.task_coordinator
                .stop_task(&thread_id, &handle, TaskStopOrigin::UserRequest, reason)
                .await?;
            let emitter = self.interaction_emitter(thread_id.clone());
            self.agent_facility
                .interactions
                .cancel_thread(&thread_id, "interrupted by user", emitter)
                .await?;
            return Ok(StudioStopPromptResponse {
                thread_id,
                stopped: true,
            });
        }
        let agent_id = self.thread_agent_path(&thread_id).await?;
        let snapshot = match handle.snapshot(agent_id.clone()).await {
            Ok(snapshot) => snapshot,
            Err(pl_core::AgentRuntimeError::NotFound(_)) => {
                return Ok(StudioStopPromptResponse {
                    thread_id,
                    stopped: false,
                });
            }
            Err(error) => return Err(anyhow::anyhow!(error)),
        };
        let Some(turn_id) = snapshot.active_turn_id else {
            return Ok(StudioStopPromptResponse {
                thread_id,
                stopped: false,
            });
        };
        match handle.cancel_turn(agent_id, turn_id).await {
            Ok(()) => {}
            Err(pl_core::AgentRuntimeError::NoActiveTurn(_))
            | Err(pl_core::AgentRuntimeError::TurnMismatch { .. }) => {
                return Ok(StudioStopPromptResponse {
                    thread_id,
                    stopped: false,
                });
            }
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
        let emitter = self.interaction_emitter(thread_id.clone());
        self.agent_facility
            .interactions
            .cancel_thread(&thread_id, "interrupted by user", emitter)
            .await?;
        Ok(StudioStopPromptResponse {
            thread_id,
            stopped: true,
        })
    }

    pub(in crate::studio) async fn ensure_thread_agent(
        &self,
        thread_id: &str,
    ) -> Result<(pl_core::AgentRuntimeHandle, pl_core::AgentId)> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        let target = self.read_owned_thread(thread_id).await?;
        let target_agent_id = pl_core::AgentId::new(target.agent_path.clone())?;
        let target_root_thread_id = target.root_thread_id.clone();
        let mut missing = Vec::new();
        let mut current = target;
        loop {
            let agent_path = pl_core::AgentId::new(current.agent_path.clone())?;
            match handle.snapshot(agent_path).await {
                Ok(_) => break,
                Err(pl_core::AgentRuntimeError::NotFound(_)) => {}
                Err(error) => return Err(anyhow::anyhow!(error)),
            }
            let parent_thread_id = current.parent_thread_id.clone();
            missing.push(current);
            let Some(parent_thread_id) = parent_thread_id else {
                break;
            };
            current = self.read_owned_thread(&parent_thread_id).await?;
        }

        for thread_record in missing.into_iter().rev() {
            let registration = self
                .thread_agent_registration(&handle, thread_record)
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
        crate::studio::agent_host::materialize_pending_task_planner_wakes(
            &handle,
            &self.store,
            Some(&target_root_thread_id),
        )
        .await?;
        Ok((handle, target_agent_id))
    }

    async fn thread_agent_registration(
        &self,
        handle: &pl_core::AgentRuntimeHandle,
        thread_record: ThreadRecord,
    ) -> Result<pl_core::AgentRegistration> {
        let agent_id = pl_core::AgentId::new(thread_record.agent_path.clone())?;
        let (parent_id, role, depth) = match thread_record.thread_kind {
            ThreadKind::Root => {
                anyhow::ensure!(
                    thread_record.parent_thread_id.is_none()
                        && agent_id == root_agent_id(&thread_record.id),
                    "root Studio Thread {} has invalid canonical owner",
                    thread_record.id
                );
                let role = match StudioMode::from_label(&thread_record.mode) {
                    StudioMode::Simple => StudioRole::Executor,
                    StudioMode::Task => StudioRole::Planner,
                };
                (None, role, 0)
            }
            ThreadKind::Agent => {
                anyhow::ensure!(
                    agent_id != root_agent_id(&thread_record.id),
                    "child Studio Thread {} cannot use a root agent identity",
                    thread_record.id
                );
                let parent_thread_id = thread_record
                    .parent_thread_id
                    .as_deref()
                    .context("child Studio Thread has no parent Thread")?;
                let parent = self.read_owned_thread(parent_thread_id).await?;
                let parent_id = pl_core::AgentId::new(parent.agent_path)?;
                let parent_snapshot = handle
                    .snapshot(parent_id.clone())
                    .await
                    .map_err(|error| anyhow::anyhow!(error))?;
                let role = StudioRole::from_key(&thread_record.role)
                    .context("child Studio Thread has an unsupported owner role")?;
                (Some(parent_id), role, parent_snapshot.identity.depth + 1)
            }
        };
        let seed = self.store.thread_runtime_seed(&thread_record.id).await?;
        let registration = pl_core::AgentRegistration {
            identity: pl_core::AgentIdentity {
                id: agent_id,
                parent_id,
                role: role.id(),
                depth,
            },
            session: pl_core::ThreadContextState {
                metadata: serde_json::json!({
                    "projectId": thread_record.project_id,
                    "title": thread_record.title,
                }),
                session: pl_core::AgentSession::new(),
                usage: pl_model::TokenUsage::default(),
                billing_by_turn: std::collections::BTreeMap::new(),
                last_context_tokens: None,
                trace_sequence: 0,
                thread_revision: seed.thread_revision,
            },
            runtime_revision: seed.runtime_revision,
            event_sequence: seed.event_sequence,
        };
        Ok(registration)
    }

    async fn read_owned_thread(&self, thread_id: &str) -> Result<ThreadRecord> {
        self.store
            .read_thread(thread_id)
            .await?
            .context("selected Thread not found")
    }

    async fn thread_agent_path(&self, thread_id: &str) -> Result<pl_core::AgentId> {
        let thread = self.read_owned_thread(thread_id).await?;
        pl_core::AgentId::new(thread.agent_path).map_err(Into::into)
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
        let thread_id = current.scope.thread_id.clone();
        if !resolution_matches_kind(&current.kind, &resolution) {
            bail!("interaction resolution kind does not match interaction");
        }
        let emitter = self.interaction_emitter(thread_id.clone());

        if current.kind == InteractionKind::PlanConfirmation {
            return self
                .resolve_plan_confirmation(interaction_id, current, resolution, emitter)
                .await;
        }

        if current.status != InteractionStatus::Pending {
            return Ok(StudioResolveInteractionResponse {
                thread_id,
                interaction: current,
                threads: Vec::new(),
            });
        }
        let resolved = if current.kind == InteractionKind::UserInput {
            let mail_id =
                pl_core::AgentInteractionContinuationRequest::stable_mail_id(&interaction_id);
            let message = serde_json::to_string_pretty(&serde_json::json!({
                "type": "studioInteractionResolution",
                "interactionId": interaction_id.clone(),
                "originTurnId": current.scope.turn_id.clone(),
                "payload": current.payload.clone(),
                "resolution": resolution.clone(),
            }))?;
            self.submit_durable_interaction_continuation(
                &current,
                resolution,
                message,
                serde_json::json!({
                    "interactionResolutionId": interaction_id,
                    "mailId": mail_id,
                    "attachmentIds": [],
                }),
            )
            .await?
        } else {
            self.agent_facility
                .interactions
                .resolve(&interaction_id, resolution, emitter)
                .await?
        };
        Ok(StudioResolveInteractionResponse {
            thread_id,
            interaction: resolved,
            threads: Vec::new(),
        })
    }

    pub(super) async fn append_plan_lifecycle_event(
        &self,
        thread_id: &str,
        plan_id: &str,
        state: PlanLifecycleState,
        turn_id: Option<String>,
        reason: Option<String>,
    ) -> Result<()> {
        // Plan 内容由 ThreadItem 持久化；生命周期由 interaction 与 Task 产品表表达。
        let _ = (thread_id, plan_id, state, turn_id, reason);
        Ok(())
    }

    pub(super) async fn record_thread_facts(
        &self,
        thread_id: &str,
        facts: Vec<pl_core::ThreadNotificationFact>,
    ) -> Result<()> {
        let runtime = self.agent_framework().await?.handle();
        let agent_path = self.thread_agent_path(thread_id).await?;
        runtime
            .record_thread_facts(
                agent_path,
                pl_core::ThreadId::new(thread_id.to_string())?,
                facts,
            )
            .await
            .map_err(Into::into)
    }

    pub(super) fn interaction_emitter(&self, thread_id: String) -> InteractionEmitter {
        let runtime = self.clone();
        Arc::new(move |interaction| {
            let runtime = runtime.clone();
            let thread_id = thread_id.clone();
            async move {
                runtime
                    .record_thread_facts(
                        &thread_id,
                        vec![pl_core::ThreadNotificationFact::durable(
                            interaction.updated_at,
                            pl_protocol::ThreadNotification::InteractionChanged {
                                interaction: Box::new(interaction),
                            },
                        )],
                    )
                    .await?;
                Ok(())
            }
            .boxed()
        })
    }

    pub(super) async fn recover_interactions_after_restart(&self) -> Result<()> {
        for interaction in self.store.list_restart_recoverable_user_inputs().await? {
            let thread_id = interaction.scope.thread_id.clone();
            let _ = self.ensure_thread_agent(&thread_id).await?;
            let emitter = self.interaction_emitter(thread_id);
            let recovered = self
                .agent_facility
                .interactions
                .recover_user_input(interaction, emitter)
                .await?;
            self.store
                .mark_restart_user_input_recovered(&recovered)
                .await?;
        }

        let mut thread_ids = self
            .store
            .list_threads_with_transient_pending_interactions()
            .await?;
        thread_ids.sort();
        thread_ids.dedup();
        for thread_id in thread_ids {
            // pending interaction 可能先于 framework registration 持久化。
            // 先恢复 canonical owner，事件仍由 PL actor 分配序列、持久化并广播。
            let (handle, _) = self.ensure_thread_agent(&thread_id).await?;
            let canonical = handle
                .thread_snapshot(&pl_core::ThreadId::new(thread_id.clone())?)
                .map_err(|error| anyhow::anyhow!(error))?;
            let emitter = self.interaction_emitter(thread_id.clone());
            for interaction in self.store.list_pending_interactions(&thread_id).await? {
                if interaction.kind == InteractionKind::ToolApproval
                    || canonical
                        .interactions
                        .iter()
                        .any(|candidate| candidate.interaction_id == interaction.interaction_id)
                {
                    continue;
                }
                emitter(interaction).await?;
            }
            self.agent_facility
                .interactions
                .cancel_recovered_tool_approvals(
                    &thread_id,
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
            "threadId": lifecycle.thread_id,
            "planId": lifecycle.plan_id,
        })
    });
    serde_json::json!({
        "attachmentIds": attachment_ids,
        "historyPolicy": "persist",
        "planLifecycle": lifecycle,
    })
}

use std::sync::Arc;

use crate::{InteractionKind, InteractionRequest, InteractionResolution, InteractionStatus};
use anyhow::{Context, Result, bail};
use futures::FutureExt;

use crate::config::StudioRole;
use crate::studio::agent_host::root_agent_id;
use crate::studio::task_coordinator::{TaskStopOrigin, TaskStopReason};
use crate::studio::{InteractionEmitter, resolution_matches_kind};
use crate::studio::{ThreadKind, ThreadRecord, ThreadVisibility};
use pl_core::ThreadRepository as _;

use super::{
    StudioResolveInteractionResponse, StudioRuntime, StudioStopPromptResponse,
    StudioSubmitPromptOptions, StudioSubmitPromptRequest, StudioSubmitPromptResponse,
};

impl StudioRuntime {
    /// Starts a new active Turn for a Thread.
    pub async fn start_turn(
        &self,
        thread_id: String,
        request: pl_protocol::studio::StartTurnRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        self.submit_prompt(StudioSubmitPromptRequest {
            thread_id,
            input: request.input,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::StartOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await
    }

    /// Steers the currently active Turn for a Thread.
    pub async fn steer_turn(
        &self,
        thread_id: String,
        request: pl_protocol::studio::SteerTurnRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        self.submit_prompt(StudioSubmitPromptRequest {
            thread_id,
            input: request.input,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::SteerOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await
    }

    pub async fn submit_prompt(
        &self,
        request: StudioSubmitPromptRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        validate_prompt_content(&request.input)?;
        self.ensure_persistence_accepts_new_work()?;
        // Serialize turn registration with the updater's final idle check.
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.submit_prompt_with_lifecycle_lock(request).await
    }

    pub(super) async fn submit_prompt_with_lifecycle_lock(
        &self,
        request: StudioSubmitPromptRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        let thread_record = self.read_owned_thread(&request.thread_id).await?;
        self.submit_prompt_for_owned_thread_with_lifecycle_lock(request, thread_record)
            .await
    }

    pub(super) async fn submit_prompt_for_owned_thread_with_lifecycle_lock(
        &self,
        request: StudioSubmitPromptRequest,
        thread_record: ThreadRecord,
    ) -> Result<StudioSubmitPromptResponse> {
        let StudioSubmitPromptRequest {
            thread_id,
            input,
            options,
        } = request;
        validate_prompt_content(&input)?;
        let pl_protocol::studio::StudioPromptInput {
            text: prompt,
            attachment_draft_ids,
        } = input;
        self.ensure_persistence_accepts_new_work()?;
        anyhow::ensure!(
            thread_record.id == thread_id,
            "prompt Thread does not match its canonical owner"
        );
        let drafts = self
            .attachment_drafts
            .resolve(&attachment_draft_ids)
            .await?;
        let role = if thread_record.parent_thread_id.is_none() {
            thread_record.mode.root_role()
        } else {
            StudioRole::from_key(&thread_record.role).context("Thread has an invalid model role")?
        };
        let config = self.config_runtime.read()?;
        let route = config.config.models.resolve(&role.id())?;
        self.attachment_drafts
            .validate_for_model(&route.model, &drafts)?;
        let attachments = self
            .store
            .promote_attachment_drafts(&thread_id, &drafts)
            .await?;
        let attachment_ids = attachments
            .iter()
            .map(|attachment| attachment.id.clone())
            .collect::<Vec<_>>();
        let thread_attachments = attachments
            .iter()
            .map(crate::studio::store::attachment::thread_attachment)
            .collect::<Vec<_>>();
        self.agent_facility
            .resources
            .insert_initial_remote_urls(attachments.iter().zip(&drafts).filter_map(
                |(attachment, draft)| {
                    draft
                        .initial_remote_url
                        .clone()
                        .map(|url| (attachment.id.clone(), url))
                },
            ))
            .await;
        let mut accepted = false;
        let result = async {
            if thread_record.mode == crate::StudioMode::Task
                && thread_record.thread_kind == ThreadKind::Root
                && !self.task_runtime.has_active_task(&thread_id).await
            {
                let project = self
                    .agent_facility
                    .product_events
                    .project_snapshot()
                    .await
                    .into_iter()
                    .find(|project| project.id == thread_record.project_id)
                    .context("Task project not found in the in-memory directory")?;
                self.task_coordinator
                    .start_task(&thread_record, &prompt, &project.path)
                    .await?;
            }
            self.ensure_prompt_runtime_ready().await?;
            let (handle, agent_id) = self
                .ensure_thread_agent_for_record(thread_record.clone())
                .await?;
            let mut snapshot = handle
                .snapshot(agent_id.clone())
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
            if matches!(
                &snapshot.state,
                pl_core::AgentState::Faulted(faulted)
                    if faulted.classification().is_recoverable()
            ) {
                snapshot = handle
                    .recover_faulted(agent_id.clone())
                    .await
                    .map_err(|error| anyhow::anyhow!(error))?;
            }
            self.reconcile_root_role(&handle, &agent_id, &thread_record, &snapshot)
                .await?;
            let thread = pl_core::ThreadId::new(thread_id.clone())?;
            let metadata = submit_metadata(&options);
            let presentation = options.presentation.clone();
            let turn_id = handle
                .submit(
                    agent_id.clone(),
                    pl_core::AgentSubmitRequest::start(thread.clone(), prompt.clone())
                        .with_presentation(presentation)
                        .with_attachments(thread_attachments)
                        .with_metadata(metadata)
                        .with_turn_policy(options.turn_policy),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
            accepted = true;
            let cursor = handle
                .thread_snapshot(&thread)
                .map_err(|error| anyhow::anyhow!(error))?
                .revision;
            Ok::<_, anyhow::Error>(StudioSubmitPromptResponse {
                thread_id,
                turn_id: turn_id.into_string(),
                cursor,
            })
        }
        .await;
        if accepted {
            self.attachment_drafts.commit(&attachment_draft_ids).await;
            return result;
        }
        self.agent_facility
            .resources
            .remove_initial_remote_urls(&attachment_ids)
            .await;
        if let Err(cleanup_error) = self
            .store
            .delete_attachments(&thread_record.id, &attachment_ids)
            .await
        {
            return Err(result
                .err()
                .unwrap_or_else(|| anyhow::anyhow!("prompt submission failed"))
                .context(format!(
                    "failed to roll back attachments: {cleanup_error:#}"
                )));
        }
        result
    }

    pub(super) async fn ensure_prompt_runtime_ready(&self) -> Result<()> {
        if !self.runtime_snapshot().await?.state.is_ready() {
            bail!("Studio runtime is not ready");
        }
        Ok(())
    }

    pub async fn stop_prompt(&self, thread_id: String) -> Result<StudioStopPromptResponse> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        if self.task_runtime.has_active_task(&thread_id).await {
            let reason = TaskStopReason::new("用户在 Studio 中请求停止任务")
                .expect("fixed user stop reason must not be empty");
            self.task_coordinator
                .stop_task(&thread_id, &handle, TaskStopOrigin::UserRequest, reason)
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
        let Some(turn_id) = snapshot.active_turn_id().cloned() else {
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
            .cancel_thread(
                self.pending_thread_interactions(&thread_id).await?,
                "interrupted by user",
                emitter,
            )
            .await?;
        Ok(StudioStopPromptResponse {
            thread_id,
            stopped: true,
        })
    }

    /// Interrupts a Turn only when the caller's expected identity still matches the active Turn.
    pub async fn interrupt_prompt(
        &self,
        thread_id: String,
        expected_turn_id: String,
    ) -> Result<super::StudioInterruptPromptResponse> {
        let snapshot = self.thread_snapshot(&thread_id).await?;
        let active_turn_id = snapshot.active_turn.as_ref().map(|turn| turn.id.as_str());
        if active_turn_id.is_some_and(|active| active != expected_turn_id) {
            return Err(anyhow::Error::new(
                pl_protocol::studio::StudioError::invalid_argument(
                    "expected Turn does not match the active Turn",
                ),
            ));
        }
        let response = self.stop_prompt(thread_id).await?;
        Ok(super::StudioInterruptPromptResponse {
            thread_id: response.thread_id,
            turn_id: expected_turn_id,
            interrupted: response.stopped,
        })
    }

    /// 把 root actor 的 `identity.role` 对齐到 mode 派生角色。mode 目录记录是
    /// canonical，actor 角色只是投影；切换后的短暂漂移在每次提交前自愈。
    /// 非 root、已一致或 actor 非 idle 时是 no-op。
    async fn reconcile_root_role(
        &self,
        handle: &pl_core::AgentRuntimeHandle,
        agent_id: &pl_core::ThreadId,
        thread: &ThreadRecord,
        snapshot: &pl_core::AgentSnapshot,
    ) -> Result<()> {
        if thread.parent_thread_id.is_some() {
            return Ok(());
        }
        let desired = thread.mode.root_role().id();
        if snapshot.identity.role == desired {
            return Ok(());
        }
        if snapshot.active_turn_id().is_some()
            || snapshot.pending_inputs > 0
            || !snapshot.state.is_idle()
        {
            return Ok(());
        }
        handle
            .reconfigure_idle_role(agent_id.clone(), desired)
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(())
    }

    pub(in crate::studio) async fn ensure_thread_agent(
        &self,
        thread_id: &str,
    ) -> Result<(pl_core::AgentRuntimeHandle, pl_core::ThreadId)> {
        let target = self.read_owned_thread(thread_id).await?;
        self.ensure_thread_agent_for_record(target).await
    }

    async fn ensure_thread_agent_for_record(
        &self,
        target: ThreadRecord,
    ) -> Result<(pl_core::AgentRuntimeHandle, pl_core::ThreadId)> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        let target_agent_id = pl_core::ThreadId::new(target.agent_path.clone())?;
        let target_root_thread_id = target.root_thread_id.clone();
        let target_thread_id = target.id.clone();
        let mut missing = Vec::new();
        let mut current = target.clone();
        loop {
            let agent_path = pl_core::ThreadId::new(current.agent_path.clone())?;
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
            self.ensure_thread_resident(&handle, thread_record).await?;
        }
        handle
            .snapshot(target_agent_id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        self.residency.touch(&target_thread_id).await;
        // 激活即入热集合：目录分页在此之后能以内存事实覆盖冷行。
        self.agent_facility
            .product_events
            .apply_thread_delta(vec![pl_protocol::Thread::from(target)], Vec::new())
            .await?;
        self.enforce_residency_limit().await;
        let _ = self.task_runtime.activate(&target_root_thread_id).await?;
        crate::studio::agent_host::materialize_pending_task_planner_wakes(
            &handle,
            &self.task_runtime,
            Some(&target_root_thread_id),
        )
        .await?;
        Ok((handle, target_agent_id))
    }

    /// 让单个缺失的 Thread 驻留：已注册过 runtime 的走 durable 恢复，
    /// 从未注册的沿用初始注册（空 session）。
    async fn ensure_thread_resident(
        &self,
        handle: &pl_core::AgentRuntimeHandle,
        thread_record: ThreadRecord,
    ) -> Result<()> {
        let registered = self
            .store
            .read_thread_runtime_revision(&thread_record.id)
            .await?
            > 0;
        if registered {
            // 共享 writer 的 repository 实例：恢复基线 seed 进进程级 writer，
            // 不构造即弃的第二 writer（design/17 §17.2）。
            let repository = self
                .persistence_repository()
                .await
                .context("Studio persistence writer is unavailable")?;
            let thread_id = pl_core::ThreadId::new(thread_record.id.clone())?;
            let Some(restored) = repository.restore_thread(&thread_id).await? else {
                anyhow::bail!(
                    "Thread {} has a corrupt durable session and cannot be activated",
                    thread_record.id
                );
            };
            match handle.restore_agent(restored).await {
                Ok(_) | Err(pl_core::AgentRuntimeError::AlreadyExists(_)) => {}
                Err(error) => return Err(anyhow::anyhow!(error)),
            }
            self.residency.touch(&thread_record.id).await;
            return Ok(());
        }
        let registration = self
            .thread_agent_registration(handle, thread_record.clone())
            .await?;
        match handle.register(registration).await {
            Ok(_) | Err(pl_core::AgentRuntimeError::AlreadyExists(_)) => {}
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
        self.residency.touch(&thread_record.id).await;
        Ok(())
    }

    async fn thread_agent_registration(
        &self,
        handle: &pl_core::AgentRuntimeHandle,
        thread_record: ThreadRecord,
    ) -> Result<pl_core::AgentRegistration> {
        let agent_id = pl_core::ThreadId::new(thread_record.agent_path.clone())?;
        let (parent_id, role, depth) = match thread_record.thread_kind {
            ThreadKind::Root => {
                anyhow::ensure!(
                    thread_record.parent_thread_id.is_none()
                        && agent_id == root_agent_id(&thread_record.id),
                    "root Studio Thread {} has invalid canonical owner",
                    thread_record.id
                );
                let role = thread_record.mode.root_role();
                (None, role, 0)
            }
            ThreadKind::Agent => {
                anyhow::ensure!(
                    agent_id != root_agent_id(&thread_record.root_thread_id),
                    "child Studio Thread {} cannot use a root agent identity",
                    thread_record.id
                );
                let parent_thread_id = thread_record
                    .parent_thread_id
                    .as_deref()
                    .context("child Studio Thread has no parent Thread")?;
                let parent = self.read_owned_thread(parent_thread_id).await?;
                let parent_id = pl_core::ThreadId::new(parent.agent_path)?;
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

    pub(in crate::studio::runtime) async fn read_owned_thread(
        &self,
        thread_id: &str,
    ) -> Result<ThreadRecord> {
        let thread = if let Some(thread) = self
            .agent_facility
            .product_events
            .thread_snapshot(thread_id)
        {
            thread
        } else {
            self.store
                .read_thread(thread_id)
                .await?
                .map(pl_protocol::Thread::from)
                .context("selected Thread not found")?
        };
        if self.recovery_issues().iter().any(|issue| {
            issue.scope == crate::StudioRecoveryIssueScope::Thread
                && issue.thread_id.as_deref() == Some(thread.root_thread_id.as_str())
        }) {
            return Err(anyhow::Error::new(pl_protocol::studio::StudioError::new(
                pl_protocol::studio::StudioErrorCode::Protocol,
                "This Thread is blocked because its durable timeline is incompatible; use the recovery cleanup action",
                false,
            )));
        }
        Ok(ThreadRecord {
            id: thread.id,
            project_id: thread.project_id,
            title: thread.title,
            mode: thread.mode.into(),
            created_at: thread.created_at,
            updated_at: thread.updated_at,
            visibility: if thread.archived {
                ThreadVisibility::Archived
            } else {
                ThreadVisibility::Active
            },
            thread_kind: if thread.parent_thread_id.is_some() {
                ThreadKind::Agent
            } else {
                ThreadKind::Root
            },
            parent_thread_id: thread.parent_thread_id,
            root_thread_id: thread.root_thread_id,
            agent_path: thread.agent_path,
            role: thread.role,
            status: thread.status,
            summary: None,
            error: None,
            runtime_updated_at: None,
        })
    }

    async fn thread_agent_path(&self, thread_id: &str) -> Result<pl_core::ThreadId> {
        let thread = self.read_owned_thread(thread_id).await?;
        pl_core::ThreadId::new(thread.agent_path).map_err(Into::into)
    }

    pub async fn resolve_interaction(
        &self,
        interaction_id: String,
        resolution: InteractionResolution,
    ) -> Result<StudioResolveInteractionResponse> {
        // 这是已经开始的 Turn 的收束入口。持久化降级只暂停新的生命周期，
        // 不能阻止用户回答、审批或确认当前交互。
        let current = self
            .read_interaction_for_resolve(&interaction_id)
            .await?
            .context("interaction not found")?;
        let thread_id = current.scope.thread_id.clone();
        if !resolution_matches_kind(current.kind(), &resolution) {
            bail!("interaction resolution kind does not match interaction");
        }
        let emitter = self.interaction_emitter(thread_id.clone());

        if current.kind() == InteractionKind::PlanConfirmation {
            let task = self
                .store
                .find_latest_task_run_for_root_thread(&thread_id)
                .await?;
            let context = crate::error_mapping::StudioDiagnosticContext {
                operation: "resolvePlanConfirmation",
                task_run_id: task.as_ref().map(|task| task.id.clone()),
                thread_id: Some(thread_id),
                turn_id: Some(current.scope.turn_id.clone()),
                interaction_id: Some(interaction_id.clone()),
                state: Some(match current.status() {
                    InteractionStatus::Pending => "pending",
                    InteractionStatus::Resolved => "resolved",
                    InteractionStatus::Cancelled => "cancelled",
                    InteractionStatus::Expired => "expired",
                }),
            };
            return self
                .resolve_plan_confirmation(interaction_id, current, resolution, emitter)
                .await
                .map_err(|error| crate::error_mapping::with_studio_diagnostics(error, context));
        }

        if current.status() != InteractionStatus::Pending {
            return Ok(StudioResolveInteractionResponse {
                thread_id,
                interaction: current,
            });
        }
        let resolved = if current.kind() == InteractionKind::UserInput {
            let mail_id =
                pl_core::AgentInteractionContinuationRequest::stable_mail_id(&interaction_id);
            let message = serde_json::to_string_pretty(&serde_json::json!({
                "type": "studioInteractionResolution",
                "interactionId": interaction_id.clone(),
                "originTurnId": current.scope.turn_id.clone(),
                "payload": current.content.clone(),
                "resolution": resolution.clone(),
            }))?;
            self.submit_durable_interaction_continuation(
                &current,
                resolution,
                message,
                serde_json::json!({
                    "interactionResolutionId": interaction_id,
                    "mailId": mail_id,
                }),
            )
            .await?
        } else {
            self.agent_facility
                .interactions
                .resolve_loaded(current, resolution, emitter)
                .await?
        };
        Ok(StudioResolveInteractionResponse {
            thread_id,
            interaction: resolved,
        })
    }

    /// 内存优先读取交互：pending 交互必须来自驻留 actor 的权威快照；
    /// 已离开快照的历史交互（非 pending）回 SQLite 冷源。
    pub(in crate::studio) async fn read_interaction_for_resolve(
        &self,
        interaction_id: &str,
    ) -> Result<Option<InteractionRequest>> {
        if let Some(framework) = self.agent_facility.framework.lock().await.clone() {
            let handle = framework.handle();
            for agent in handle.directory_snapshot().agents {
                let Ok(snapshot) = handle.thread_snapshot(&agent.identity.id) else {
                    continue;
                };
                if let Some(found) = snapshot
                    .interactions
                    .iter()
                    .find(|candidate| candidate.interaction_id == interaction_id)
                {
                    return Ok(Some(found.clone()));
                }
            }
        }
        self.store.read_interaction(interaction_id).await
    }

    /// 读取驻留线程的 pending 交互；未驻留线程没有 pending 交互
    /// （钉住集合恢复 + LRU 空闲淘汰不变量）。
    pub(in crate::studio) async fn pending_thread_interactions(
        &self,
        thread_id: &str,
    ) -> Result<Vec<InteractionRequest>> {
        let Some((handle, agent_id)) = self.try_get_thread_handle(thread_id).await? else {
            return Ok(Vec::new());
        };
        match handle.thread_snapshot(&agent_id) {
            Ok(snapshot) => Ok(snapshot.interactions),
            Err(pl_core::AgentRuntimeError::NotFound(_)) => Ok(Vec::new()),
            Err(error) => Err(anyhow::anyhow!(error)),
        }
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
                if interaction.kind() == InteractionKind::ToolApproval
                    || canonical
                        .interactions
                        .iter()
                        .any(|candidate| candidate.interaction_id == interaction.interaction_id)
                {
                    continue;
                }
                emitter(interaction).await?;
            }
            let canonical = handle
                .thread_snapshot(&pl_core::ThreadId::new(thread_id.clone())?)
                .map_err(|error| anyhow::anyhow!(error))?;
            let root_thread_id = self
                .store
                .read_thread(&thread_id)
                .await?
                .with_context(|| format!("recovered Thread {thread_id} is missing"))?
                .root_thread_id;
            let task_is_terminal = self
                .store
                .find_latest_task_run_for_root_thread(&root_thread_id)
                .await?
                .is_some_and(|task| task.kind().is_terminal());
            if task_is_terminal {
                self.agent_facility
                    .interactions
                    .cancel_thread(canonical.interactions, "task completed", emitter)
                    .await?;
                continue;
            }
            self.agent_facility
                .interactions
                .cancel_recovered_tool_approvals(
                    canonical.interactions,
                    "application restarted before approval completed",
                    emitter,
                )
                .await?;
        }
        Ok(())
    }
}

pub(super) fn validate_prompt_content(
    input: &pl_protocol::studio::StudioPromptInput,
) -> Result<()> {
    if input.text.trim().is_empty() && input.attachment_draft_ids.is_empty() {
        bail!("prompt is empty");
    }
    Ok(())
}

fn submit_metadata(options: &StudioSubmitPromptOptions) -> serde_json::Value {
    let lifecycle = options.lifecycle.as_ref().map(|lifecycle| {
        serde_json::json!({
            "threadId": lifecycle.thread_id,
            "planId": lifecycle.plan_id,
        })
    });
    serde_json::json!({
        "historyPolicy": "persist",
        "planLifecycle": lifecycle,
    })
}

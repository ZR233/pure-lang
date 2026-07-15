use super::*;

impl StudioRuntime {
    pub(super) async fn run_prompt_inner(
        &self,
        request: RunPromptRequest,
        history_policy: PromptHistoryPolicy,
    ) -> Result<StudioPromptOutcome> {
        let RunPromptRequest {
            session_id,
            turn_id,
            prompt,
            attachment_ids,
            interaction_callback,
            interaction_emitter,
            mut options,
        } = request;
        let session_id = session_id.as_str();
        let session_record = self
            .store
            .read_session(session_id)
            .await?
            .context("selected session not found")?;
        let project = self
            .store
            .read_project(&session_record.project_id)
            .await?
            .context("selected project not found")?;
        let mut session = self.store.load_core_session(session_id).await?;
        let config = self.config_store.load_or_default()?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        let workspace_instructions = load_workspace_instructions(&workspace_root)?;
        let previous_revision = session.revision();
        let previous_len = session.len();
        let mode = CompileMode::from_label(&session_record.mode);
        let root_role = root_model_role(mode);
        options = options.with_permission_mode(config.runtime.permission_mode);
        let selected_attachments = self
            .store
            .load_attachments(session_id, &attachment_ids)
            .await?;
        let selected_materialized = self
            .store
            .materialize_attachments(session_id, &attachment_ids)
            .await?;
        let trace_attachments = selected_attachments
            .iter()
            .map(|record| {
                let mut attachment = crate::studio::store::attachment::trace_attachment(record);
                attachment.data_url = selected_materialized
                    .iter()
                    .find(|materialized| materialized.attachment_id == record.id)
                    .map(|materialized| {
                        format!(
                            "data:{};base64,{}",
                            materialized.media_type, materialized.data
                        )
                    });
                attachment
            })
            .collect::<Vec<_>>();
        let mut materialized_attachments = self
            .store
            .materialize_session_attachments(session_id)
            .await?;
        for attachment in selected_materialized {
            if !materialized_attachments
                .iter()
                .any(|existing| existing.attachment_id == attachment.attachment_id)
            {
                materialized_attachments.push(attachment);
            }
        }
        let user_content = prompt_content(&prompt, &selected_attachments);
        let mut request = TurnRequest::new(prompt.clone(), mode)
            .with_turn_id(turn_id.clone())
            .with_user_content(user_content)
            .with_materialized_attachments(materialized_attachments)
            .with_trace_attachments(trace_attachments);
        if !workspace_instructions.trim().is_empty() {
            request = request.with_workspace_instructions(workspace_instructions.clone());
        }
        let instruction_snapshot = self
            .resolve_instruction_snapshot(
                session_id,
                session_record.instruction_snapshot.as_ref(),
                &config,
                &workspace_root,
                Path::new(&project.path),
                mode,
            )
            .await?;
        request = request.with_instruction_snapshot(instruction_snapshot);
        self.mcp_runtime
            .reconcile(crate::config::effective_mcp_servers(&config))
            .await;
        self.lsp_runtime.reconcile_workspace(&workspace_root).await;

        let task_run = match mode {
            CompileMode::Simple => None,
            CompileMode::Task => {
                self.store
                    .find_active_task_run_for_session(session_id)
                    .await?
            }
        };
        let task_supervisor = self
            .task_agent_runtimes
            .supervisor_for_mode_generation(
                mode,
                session_id,
                &workspace_root,
                self.lifecycle_epoch(),
                task_run.as_ref().map(|run| run.id.as_str()),
            )
            .await?;
        let mut core = PureCore::from_config(&config, root_role)?
            .with_mcp_runtime(self.mcp_runtime.clone())
            .with_lsp_runtime(self.lsp_runtime.clone());
        if let Some(supervisor) = task_supervisor {
            core = core.with_agent_supervisor(supervisor);
        }
        core.register_default_tools(workspace_root.clone(), Some(workspace_instructions.clone()))
            .await;
        if mode == CompileMode::Task {
            self.task_coordinator.install_tools(&mut core, session_id);
        }
        core.register_available_mcp_tools().await?;
        if options.interaction_callback.is_none()
            && (options.requires_user_approval_callback() || mode == CompileMode::Task)
        {
            options.interaction_callback = Some(interaction_callback.clone());
        }
        let (event_tx, event_rx) = tokio::sync::broadcast::channel(4096);
        let event_runtime = self.clone();
        let event_session_id = session_id.to_string();
        let event_turn_id = turn_id.clone();
        let event_lifecycle_epoch = self.lifecycle_epoch();
        let event_task = tokio::spawn(async move {
            event_runtime
                .drain_prompt_agent_events_for_epoch(
                    event_session_id,
                    event_turn_id,
                    event_lifecycle_epoch,
                    event_rx,
                )
                .await;
        });
        let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx.clone(), 0);
        let result = core
            .run_turn_with_trace(&mut session, request, &mut recorder, options)
            .await;
        drop(recorder);
        drop(event_tx);
        let _ = event_task.await;
        let result = result?;
        let trace_events = result.trace_events.clone();
        let _post_turn_guard = self.post_turn_lock.lock().await;
        if !matches!(
            self.runtime_snapshot().status,
            crate::StudioRuntimeStatus::Ready
        ) || !self.active_turns.contains_exact(session_id, &turn_id).await
        {
            let messages = self.store.load_messages(session_id).await?;
            return Ok(StudioPromptOutcome {
                result,
                messages,
                trace_events,
            });
        }
        match history_policy {
            PromptHistoryPolicy::Persist if session.revision() != previous_revision => {
                self.store
                    .replace_turn_context_records(session_id, &trace_events, session.items())
                    .await?;
            }
            PromptHistoryPolicy::Persist => {
                let new_items = &session.items()[previous_len..];
                self.store
                    .append_turn_context_records(session_id, &trace_events, new_items)
                    .await?;
            }
            PromptHistoryPolicy::Ephemeral => {
                self.store
                    .append_turn_records(session_id, &trace_events, &[])
                    .await?;
            }
        }
        if history_policy == PromptHistoryPolicy::Persist
            && matches!(mode, CompileMode::Task)
            && matches!(result.status, TurnResultStatus::Completed)
            && let Some(plan) = completed_plan_item(&trace_events)
        {
            self.create_plan_confirmation(session_id, &plan, interaction_emitter)
                .await?;
        }
        let resolved = config.resolve_role(root_role)?;
        let model = resolved
            .models
            .iter()
            .find(|model| model.slug == result.model)
            .or_else(|| {
                resolved
                    .models
                    .iter()
                    .find(|model| model.slug == resolved.role_config.model)
            })
            .or_else(|| resolved.models.first());
        self.store
            .upsert_session_runtime_for_turn(session_id, &turn_id, &result, model)
            .await?;
        if history_policy == PromptHistoryPolicy::Persist
            && should_start_self_learning(&config, mode, &result.status, &trace_events)
        {
            let review_messages = session.messages().to_vec();
            spawn_self_learning_review(
                config.clone(),
                workspace_root.clone(),
                workspace_instructions.clone(),
                review_messages,
            );
        }
        if history_policy == PromptHistoryPolicy::Persist && previous_len == 0 {
            self.store
                .rename_session(session_id, &session_title_from_prompt(&prompt))
                .await?;
        }
        let messages = self.store.load_messages(session_id).await?;
        Ok(StudioPromptOutcome {
            result,
            messages,
            trace_events,
        })
    }

    async fn resolve_instruction_snapshot(
        &self,
        session_id: &str,
        existing: Option<&InstructionSnapshot>,
        config: &crate::config::PureConfig,
        workspace_root: &Path,
        project_path: &Path,
        mode: CompileMode,
    ) -> Result<InstructionSnapshot> {
        let resolved = config.resolve_role(root_model_role(mode))?;
        let model = resolved
            .models
            .iter()
            .find(|model| model.slug == resolved.role_config.model)
            .cloned()
            .unwrap_or_else(|| pl_model::ModelInfo::fallback(&resolved.role_config.model));
        let current_dir =
            std::fs::canonicalize(project_path).unwrap_or_else(|_| workspace_root.to_path_buf());
        if let Some(snapshot) = existing {
            return Ok(snapshot.with_turn_overlay(InstructionAssemblyRequest {
                config: Some(config),
                model: &model,
                mode,
                workspace_root,
                current_dir: &current_dir,
                workspace_instructions: None,
                subagent_constraint: None,
            })?);
        }
        let snapshot = InstructionAssembler::assemble(InstructionAssemblyRequest {
            config: Some(config),
            model: &model,
            mode,
            workspace_root,
            current_dir: &current_dir,
            workspace_instructions: None,
            subagent_constraint: None,
        })?;
        self.store
            .save_instruction_snapshot(session_id, &snapshot)
            .await?
            .context("selected session disappeared while saving instruction snapshot")?;
        Ok(snapshot)
    }
}

fn root_model_role(mode: CompileMode) -> ModelRole {
    match mode {
        CompileMode::Simple => ModelRole::Executor,
        CompileMode::Task => ModelRole::Planner,
    }
}

fn completed_plan_item(events: &[TraceEvent]) -> Option<TracePart> {
    events.iter().rev().find_map(|event| match &event.kind {
        TraceEventKind::TracePartCompleted { item }
            if item.kind == TracePartKind::Plan && !item.content.trim().is_empty() =>
        {
            Some(item.clone())
        }
        TraceEventKind::TracePartStarted { .. }
        | TraceEventKind::TracePartDelta { .. }
        | TraceEventKind::TracePartCompleted { .. }
        | TraceEventKind::TracePartFailed { .. }
        | TraceEventKind::PlanLifecycleChanged { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => None,
    })
}

fn prompt_content(prompt: &str, attachments: &[crate::studio::AttachmentRecord]) -> MessageContent {
    if attachments.is_empty() {
        return MessageContent::Text(prompt.to_string());
    }
    let mut parts = Vec::new();
    if !prompt.is_empty() {
        parts.push(ContentPart::Text {
            text: prompt.to_string(),
        });
    }
    parts.extend(attachments.iter().map(|attachment| ContentPart::Image {
        source: ImageSource::Attachment {
            attachment_id: attachment.id.clone(),
        },
        media_type: attachment.media_type.clone(),
        filename: attachment.filename.clone(),
    }));
    MessageContent::MultiPart(parts)
}

fn session_title_from_prompt(prompt: &str) -> String {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return "新会话".to_string();
    }
    prompt.chars().take(42).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_modes_route_to_their_root_roles() {
        assert_eq!(root_model_role(CompileMode::Simple), ModelRole::Executor);
        assert_eq!(root_model_role(CompileMode::Task), ModelRole::Planner);
    }
}

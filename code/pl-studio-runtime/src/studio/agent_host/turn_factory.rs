use std::path::Path;
use std::sync::Arc;

use crate::{ContentPart, ImageSource, MessageContent, PureError, Result};
use pl_core::{
    AgentCollaborationTools, AgentIdentity, AgentTurnFactory, AgentTurnPreparationContext,
    CoreRuntimeProfile, ExecutionInstructionProfile, InstructionAssembler,
    InstructionAssemblyRequest, PreparedAgentTurn, PreparedSessionRuntime, SubagentContext,
    ToolVisibilitySet, TurnEngineBuilder, TurnOptions, TurnRequest, load_workspace_instructions,
    plan_web_search,
};
use pl_model::create_provider_with_catalog;

use crate::config::ConfigStore;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{InteractionRuntime, StudioStore};
use crate::{McpRuntimeHandle, StudioMode};

use super::policy::{StudioPolicyContext, studio_execution_policy};
use super::resources::StudioAgentResources;
use super::workspace_resolver::AgentWorkspaceResolver;

/// 使用 Studio 配置、project/session 和产品工具准备一次 framework turn。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentTurnFactory {
    store: StudioStore,
    config_store: ConfigStore,
    mcp_runtime: McpRuntimeHandle,
    lsp_runtime: pl_lsp::LspRuntimeRegistry,
    interactions: InteractionRuntime,
    coordinator: Arc<TaskCoordinator>,
    resources: StudioAgentResources,
}

impl StudioAgentTurnFactory {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        store: StudioStore,
        config_store: ConfigStore,
        mcp_runtime: McpRuntimeHandle,
        lsp_runtime: pl_lsp::LspRuntimeRegistry,
        interactions: InteractionRuntime,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
    ) -> Self {
        Self {
            store,
            config_store,
            mcp_runtime,
            lsp_runtime,
            interactions,
            coordinator,
            resources,
        }
    }
}

impl AgentTurnFactory for StudioAgentTurnFactory {
    type Error = PureError;

    async fn prepare_turn(
        &self,
        context: AgentTurnPreparationContext,
    ) -> Result<PreparedAgentTurn> {
        let thread_id = self
            .resources
            .thread_id(&context.snapshot.identity.id)
            .await
            .ok_or_else(|| turn_error("agent has no Studio Thread boundary"))?;
        let thread_record = self
            .store
            .read_thread(&thread_id)
            .await
            .map_err(anyhow_error)?
            .ok_or_else(|| turn_error("selected Studio Thread not found"))?;
        let project = self
            .store
            .read_project(&thread_record.project_id)
            .await
            .map_err(anyhow_error)?
            .ok_or_else(|| turn_error("selected Studio project not found"))?;
        let config = self.config_store.load_or_default()?;
        let mode = StudioMode::from_label(&thread_record.mode);
        ensure_root_role_matches_mode(&context.snapshot.identity, mode)?;
        let active_task_run = if mode == StudioMode::Task {
            self.store
                .find_active_task_run_for_root_thread(&thread_record.root_thread_id)
                .await
                .map_err(anyhow_error)?
        } else {
            None
        };
        let task_phase = active_task_run.as_ref().map(|run| run.phase);
        if let Some(run) = active_task_run.as_ref() {
            ensure_task_accepts_turn(run)?;
        }
        let workspace = AgentWorkspaceResolver::new(self.store.clone())
            .resolve(
                &context.snapshot.identity,
                &thread_record,
                &project,
                active_task_run.as_ref(),
            )
            .await
            .map_err(anyhow_error)?;
        let workspace_root = workspace.root().to_path_buf();
        let workspace_instructions =
            load_workspace_instructions(&workspace_root).map_err(anyhow_error)?;
        let executor_handoff = if mode == StudioMode::Task
            && context.snapshot.identity.parent_id.is_some()
            && context.snapshot.identity.role.as_str() == crate::config::StudioRole::Executor.key()
        {
            let run = active_task_run
                .as_ref()
                .ok_or_else(|| turn_error("Task executor has no active TaskRun"))?;
            let work_unit = self
                .store
                .find_work_unit_for_executor(context.snapshot.identity.id.as_str())
                .await
                .map_err(anyhow_error)?
                .ok_or_else(|| turn_error("Task executor has no durable WorkUnit"))?;
            let section = context
                .session
                .pinned_context_sections()
                .find(|section| {
                    section.id.as_str()
                        == crate::studio::task_coordinator::TASK_EXECUTOR_HANDOFF_SECTION_ID
                })
                .cloned();
            let validated = section
                .ok_or_else(|| anyhow::anyhow!("Task executor handoff is missing"))
                .and_then(|section| {
                    let handoff = crate::studio::task_coordinator::TaskExecutorHandoffV1::from_context_section(
                        &section,
                    )?;
                    handoff.validate_owner(
                        run,
                        &work_unit,
                        context.snapshot.identity.id.as_str(),
                    )?;
                    Ok(section)
                });
            let section = match validated {
                Ok(section) => section,
                Err(error) => {
                    let message = error.to_string();
                    self.store
                        .mark_executor_handoff_needs_attention(
                            context.snapshot.identity.id.as_str(),
                            &message,
                        )
                        .await
                        .map_err(anyhow_error)?;
                    return Err(turn_error(message));
                }
            };
            self.store
                .mark_executor_turn_started(context.snapshot.identity.id.as_str())
                .await
                .map_err(anyhow_error)?;
            Some(section)
        } else {
            None
        };
        let input_message = context.input.message.clone();
        let model_role = if context.snapshot.identity.parent_id.is_none() {
            match mode {
                StudioMode::Simple => crate::config::StudioRole::Executor.id(),
                StudioMode::Task => crate::config::StudioRole::Planner.id(),
            }
        } else {
            context.snapshot.identity.role.clone()
        };
        let route = config.models.resolve(&model_role)?;
        let web_search = plan_web_search(&config.models, &route, &config.web_search)?;
        let provider = create_provider_with_catalog(route.provider_info, route.models)?;
        let mut builder = TurnEngineBuilder::new(provider)
            .with_tool_capabilities(config.runtime.tool_capabilities.clone())
            .with_skills_config(config.skills.clone())
            .with_lsp_runtime(self.lsp_runtime.clone());
        if let Some(effort) = route.effort {
            builder = builder.with_effort(effort);
        }
        let profile = CoreRuntimeProfile::local_agent_workspace(workspace.clone())
            .with_workspace_instructions(workspace_instructions.clone());
        let task_name = self
            .resources
            .get(&context.snapshot.identity.id)
            .await
            .map(|resource| resource.task_name)
            .unwrap_or_else(|| context.snapshot.identity.role.to_string());
        let subagent_context = runtime_subagent_context(&context.snapshot.identity, task_name);
        let mut engine = builder.with_runtime_profile(profile).build();
        if let Some(subagent) = subagent_context {
            engine = engine.with_subagent_context(subagent);
        }
        self.lsp_runtime.reconcile_workspace(&workspace_root).await;
        engine.register_profile_tools().await;

        web_search.install(&mut engine, &config.web_search)?;

        if mode == StudioMode::Task {
            self.coordinator.install_tools(
                &mut engine,
                &thread_record.root_thread_id,
                context.runtime.clone(),
                &context.snapshot,
            );
        }
        self.mcp_runtime
            .reconcile(crate::config::effective_mcp_servers(&config))
            .await?;
        let mcp_lease = self.mcp_runtime.acquire_turn_lease().await?;
        let active_mcp_servers = mcp_lease.server_ids().to_vec();
        let mcp_health = self.mcp_runtime.health_snapshot().await?;
        let active_lsp_servers = self
            .lsp_runtime
            .active_server_names_for_workspace(&workspace_root)
            .await;
        mcp_lease.install(&mut engine)?;

        let mut policy = studio_execution_policy(
            &context.snapshot,
            StudioPolicyContext { mode, task_phase },
            ToolVisibilitySet::from_tool_names(engine.tool_names()),
        );
        policy.visible_tools = web_search.constrain_visibility(policy.visible_tools);
        let collaboration = AgentCollaborationTools::new(
            context.runtime.clone(),
            context.snapshot.identity.id.clone(),
            policy.collaboration.clone(),
        );
        let collaboration_tools =
            if mode == StudioMode::Task && context.snapshot.identity.parent_id.is_none() {
                collaboration.tools_without_send_message()
            } else {
                collaboration.tools()
            };
        for tool in collaboration_tools {
            engine.register_tool(tool);
        }

        let attachment_ids = context
            .input
            .metadata
            .get("attachmentIds")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let attachments = self
            .store
            .load_attachments(&thread_id, &attachment_ids)
            .await
            .map_err(anyhow_error)?;
        let materialized = self
            .store
            .materialize_thread_attachments(&thread_id)
            .await
            .map_err(anyhow_error)?;
        let user_content = prompt_content(&input_message, &attachments);
        let trace_attachments = attachments
            .iter()
            .map(crate::studio::store::attachment::trace_attachment)
            .collect();
        let instruction_snapshot = instruction_snapshot(
            &config,
            &route.model,
            mode,
            &workspace_root,
            &workspace_instructions,
            context
                .input
                .metadata
                .get("subagentConstraint")
                .and_then(serde_json::Value::as_str),
        )?;
        let request = TurnRequest::new(input_message)
            .with_turn_id(context.turn_id.to_string())
            .with_user_content(user_content)
            .with_materialized_attachments(materialized)
            .with_trace_attachments(trace_attachments)
            .with_workspace_instructions(workspace_instructions)
            .with_instruction_snapshot(instruction_snapshot);
        let emitter = interaction_emitter(
            context.runtime,
            thread_id.clone(),
            context.snapshot.identity.id.to_string(),
        );
        let interaction_callback = self.interactions.callback(thread_id, emitter);
        let prompt_cache_namespace = context.snapshot.identity.id.to_string();
        let options = TurnOptions::default()
            .with_permission_mode(config.runtime.permission_mode)
            .with_prompt_cache_namespace(prompt_cache_namespace)
            .with_prompt_scope(format!(
                "{}:{}",
                mode.label(),
                context.snapshot.identity.role
            ))
            .with_interaction_callback(interaction_callback);
        let mut session_runtime = PreparedSessionRuntime::new(route.model.slug.clone())
            .with_mcp_servers(active_mcp_servers)
            .with_mcp_health(mcp_health)
            .with_lsp(active_lsp_servers);
        if let Some(context_window) = route.model.resolved_context_window() {
            session_runtime = session_runtime.with_context_window(context_window);
        }
        let mut prepared = PreparedAgentTurn::new(engine, request, options, policy)
            .with_session_runtime(session_runtime);
        if let Some(handoff) = executor_handoff {
            prepared = prepared.with_pinned_context(handoff);
        }
        if context
            .input
            .metadata
            .get("historyPolicy")
            .and_then(serde_json::Value::as_str)
            == Some("ephemeral")
        {
            Ok(prepared.with_session_commit(pl_core::AgentSessionCommitPolicy::DiscardTurn))
        } else {
            Ok(prepared)
        }
    }
}

fn instruction_snapshot(
    config: &crate::config::StudioConfig,
    model: &pl_model::ModelInfo,
    mode: StudioMode,
    workspace_root: &Path,
    workspace_instructions: &str,
    subagent_constraint: Option<&str>,
) -> Result<pl_core::InstructionSnapshot> {
    InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&config.instructions),
        skills: Some(&config.skills),
        execution_profile: Some(ExecutionInstructionProfile {
            label: mode.label(),
            instructions: mode.instructions(),
        }),
        model,
        workspace_root,
        current_dir: workspace_root,
        workspace_instructions: Some(workspace_instructions),
        subagent_constraint,
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

fn interaction_emitter(
    runtime: pl_core::AgentRuntimeHandle,
    thread_id: String,
    agent_path: String,
) -> crate::studio::InteractionEmitter {
    Arc::new(move |interaction| {
        let runtime = runtime.clone();
        let thread_id = thread_id.clone();
        let agent_path = agent_path.clone();
        Box::pin(async move {
            let emitted_at = interaction.updated_at;
            runtime
                .record_thread_facts(
                    pl_core::AgentId::new(agent_path.clone())?,
                    pl_core::ThreadId::new(thread_id)?,
                    vec![pl_core::ThreadNotificationFact::durable(
                        emitted_at,
                        pl_protocol::ThreadNotification::InteractionChanged {
                            interaction: Box::new(interaction),
                        },
                    )],
                )
                .await?;
            Ok(())
        })
    })
}

fn turn_error(error: impl Into<String>) -> PureError {
    PureError::MemoryError(error.into())
}

fn runtime_subagent_context(identity: &AgentIdentity, task: String) -> Option<SubagentContext> {
    let parent_id = identity.parent_id.as_ref()?.to_string();
    Some(SubagentContext {
        id: identity.id.to_string(),
        parent_id: Some(parent_id),
        agent_path: Some(format!("/root/{}", identity.id)),
        role: identity.role.to_string(),
        task,
        depth: identity.depth,
    })
}

fn ensure_task_accepts_turn(run: &crate::studio::task_coordinator::TaskRunRecord) -> Result<()> {
    if run.stop_requested {
        return Err(turn_error("task is quiescing; no new turn may start"));
    }
    Ok(())
}

fn ensure_root_role_matches_mode(identity: &AgentIdentity, mode: StudioMode) -> Result<()> {
    if identity.parent_id.is_some() {
        return Ok(());
    }
    let expected = match mode {
        StudioMode::Simple => crate::config::StudioRole::Executor,
        StudioMode::Task => crate::config::StudioRole::Planner,
    };
    if identity.role != expected.id() {
        return Err(turn_error(format!(
            "root Studio Thread role {} does not match {} mode role {}",
            identity.role,
            mode.label(),
            expected.key()
        )));
    }
    Ok(())
}

fn anyhow_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::task_coordinator::{
        TaskRunPhase, TaskRunRecord, TaskStopOrigin, TaskStopReason,
    };

    #[test]
    fn stop_requested_task_rejects_every_new_turn() {
        let run = stopped_run();
        assert!(ensure_task_accepts_turn(&run).is_err());
    }

    #[test]
    fn task_root_rejects_stale_executor_identity() {
        let identity = AgentIdentity {
            id: pl_core::AgentId::new("root").unwrap(),
            parent_id: None,
            role: crate::config::StudioRole::Executor.id(),
            depth: 0,
        };

        let error = ensure_root_role_matches_mode(&identity, StudioMode::Task).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not match task mode role planner")
        );
    }

    fn stopped_run() -> TaskRunRecord {
        TaskRunRecord {
            id: "task-run".to_string(),
            root_thread_id: "session".to_string(),
            phase: TaskRunPhase::Implementing,
            plan: "plan".to_string(),
            workspace_root: "C:/workspace".to_string(),
            git_common_dir: "C:/workspace/.git".to_string(),
            branch: "main".to_string(),
            base_commit: "base".to_string(),
            expected_head: "head".to_string(),
            design_commit: Some("head".to_string()),
            status_message: None,
            stop_requested: true,
            stop_requested_origin: Some(TaskStopOrigin::UserRequest),
            stop_requested_reason: TaskStopReason::new("stop"),
            stop_requested_at: Some(1),
            task_generation: 7,
            terminal_generation: None,
            terminal_failure_id: None,
            created_at: 1,
            updated_at: 1,
        }
    }
}

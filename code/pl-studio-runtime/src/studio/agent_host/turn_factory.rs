use std::path::Path;
use std::sync::Arc;

use crate::{ContentPart, ImageSource, MessageContent, PureError, Result};
use pl_core::{
    AgentCollaborationTools, AgentIdentity, AgentKernel, AgentTurnFactory,
    AgentTurnPreparationContext, CoreAgentProfile, ExecutionInstructionProfile,
    InstructionAssembler, InstructionAssemblyRequest, PreparedAgentTurn, PreparedSessionRuntime,
    SubagentContext, ToolVisibilitySet, TurnEngineBuilder, TurnOptions, TurnRequest,
    load_workspace_instructions, plan_web_search,
};
use pl_model::create_provider_with_catalog;

use crate::config::ConfigStore;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{InteractionRuntime, StudioStore};
use crate::{McpRuntimeHandle, StudioMode, resolve_workspace_root};

use super::policy::{StudioPolicyContext, studio_execution_policy};
use super::resources::StudioAgentResources;

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
        let studio_session_id = self
            .resources
            .studio_session_id(&context.snapshot.identity.id)
            .await
            .ok_or_else(|| turn_error("agent has no Studio session boundary"))?;
        let session_record = self
            .store
            .read_session(&studio_session_id)
            .await
            .map_err(anyhow_error)?
            .ok_or_else(|| turn_error("selected Studio session not found"))?;
        let project = self
            .store
            .read_project(&session_record.project_id)
            .await
            .map_err(anyhow_error)?
            .ok_or_else(|| turn_error("selected Studio project not found"))?;
        let project_root =
            resolve_workspace_root(Path::new(&project.path)).map_err(anyhow_error)?;
        let workspace_root = self
            .resources
            .workspace_root(&context.snapshot.identity.id)
            .await
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(project_root);
        let workspace_instructions =
            load_workspace_instructions(&workspace_root).map_err(anyhow_error)?;
        let config = self.config_store.load_or_default()?;
        let mode = StudioMode::from_label(&session_record.mode);
        let active_task_run = if mode == StudioMode::Task {
            self.store
                .find_active_task_run_for_session(&session_record.root_session_id)
                .await
                .map_err(anyhow_error)?
        } else {
            None
        };
        let task_phase = active_task_run.as_ref().map(|run| run.phase);
        if let Some(run) = active_task_run.as_ref() {
            ensure_task_accepts_turn(run)?;
        }
        if mode == StudioMode::Task
            && context.snapshot.identity.role.as_str() == crate::config::StudioRole::Executor.key()
        {
            self.store
                .mark_executor_turn_started(context.snapshot.identity.id.as_str())
                .await
                .map_err(anyhow_error)?;
        }
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
        let profile = CoreAgentProfile::local_workspace(workspace_root.clone())
            .with_workspace_instructions(workspace_instructions.clone());
        let task_name = self
            .resources
            .get(&context.snapshot.identity.id)
            .await
            .map(|resource| resource.task_name)
            .unwrap_or_else(|| context.snapshot.identity.role.to_string());
        let subagent_context = runtime_subagent_context(&context.snapshot.identity, task_name);
        let kernel_builder = AgentKernel::builder(builder).with_profile(profile);
        let mut kernel = match subagent_context {
            Some(subagent) => kernel_builder.with_subagent_context(subagent).build().await,
            None => kernel_builder.build().await,
        };

        web_search.install(kernel.core_mut(), &config.web_search)?;

        if mode == StudioMode::Task {
            self.coordinator.install_tools(
                kernel.core_mut(),
                &session_record.root_session_id,
                context.runtime.clone(),
                &context.snapshot,
            );
        }
        self.mcp_runtime
            .reconcile(crate::config::effective_mcp_servers(&config))
            .await?;
        self.lsp_runtime.reconcile_workspace(&workspace_root).await;
        let mcp_lease = self.mcp_runtime.acquire_turn_lease().await?;
        let active_mcp_servers = mcp_lease.server_ids().to_vec();
        let mcp_health = self.mcp_runtime.health_snapshot().await?;
        let active_lsp_servers = self.lsp_runtime.active_server_names().await;
        mcp_lease.install(kernel.core_mut())?;

        let mut policy = studio_execution_policy(
            &context.snapshot,
            StudioPolicyContext { mode, task_phase },
            ToolVisibilitySet::from_tool_names(kernel.tool_names()),
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
            kernel.core_mut().register_tool(tool);
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
            .load_attachments(&studio_session_id, &attachment_ids)
            .await
            .map_err(anyhow_error)?;
        let materialized = self
            .store
            .materialize_session_attachments(&studio_session_id)
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
            Path::new(&project.path),
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
            studio_session_id.clone(),
            context.snapshot.identity.id.to_string(),
        );
        let interaction_callback = self.interactions.callback(studio_session_id, emitter);
        let options = TurnOptions::default()
            .with_permission_mode(config.runtime.permission_mode)
            .with_interaction_callback(interaction_callback);
        let mut session_runtime = PreparedSessionRuntime::new(route.model.slug.clone())
            .with_mcp_servers(active_mcp_servers)
            .with_mcp_health(mcp_health)
            .with_lsp(active_lsp_servers);
        if let Some(context_window) = route.model.resolved_context_window() {
            session_runtime = session_runtime.with_context_window(context_window);
        }
        let prepared = PreparedAgentTurn::new(kernel, request, options, policy)
            .with_session_runtime(session_runtime);
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
    project_path: &Path,
    workspace_instructions: &str,
    subagent_constraint: Option<&str>,
) -> Result<pl_core::InstructionSnapshot> {
    let current_dir =
        std::fs::canonicalize(project_path).unwrap_or_else(|_| workspace_root.to_path_buf());
    InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&config.instructions),
        skills: Some(&config.skills),
        execution_profile: Some(ExecutionInstructionProfile {
            label: mode.label(),
            instructions: mode.instructions(),
        }),
        model,
        workspace_root,
        current_dir: &current_dir,
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
    session_id: String,
    owner_agent_id: String,
) -> crate::studio::InteractionEmitter {
    Arc::new(move |interaction| {
        let runtime = runtime.clone();
        let session_id = session_id.clone();
        let owner_agent_id = owner_agent_id.clone();
        Box::pin(async move {
            let turn_id = interaction.scope.turn_id.clone();
            let emitted_at = interaction.updated_at;
            runtime
                .record_session_facts(
                    pl_core::AgentId::new(owner_agent_id.clone())?,
                    pl_core::SessionId::new(session_id)?,
                    vec![pl_core::SessionEventFact::durable(
                        Some(owner_agent_id),
                        Some(turn_id),
                        emitted_at,
                        crate::SessionEventKind::InteractionChanged {
                            event: Box::new(crate::InteractionChangedEvent { interaction }),
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

    fn stopped_run() -> TaskRunRecord {
        TaskRunRecord {
            id: "task-run".to_string(),
            session_id: "session".to_string(),
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
            created_at: 1,
            updated_at: 1,
        }
    }
}

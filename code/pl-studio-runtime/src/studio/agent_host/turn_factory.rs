use std::path::Path;
use std::sync::Arc;

use crate::{ContentPart, ImageSource, MessageContent, PureError, Result};
use pl_core::{
    AgentCollaborationTools, AgentKernel, AgentTurnFactory, AgentTurnPreparationContext,
    CoreAgentProfile, ExecutionInstructionProfile, InstructionAssembler,
    InstructionAssemblyRequest, PreparedAgentTurn, SubagentContext, ToolVisibilitySet,
    TurnEngineBuilder, TurnOptions, TurnRequest, load_workspace_instructions, plan_web_search,
};
use pl_model::create_provider_with_catalog;

use crate::config::ConfigStore;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{InteractionRuntime, StudioEventRuntime, StudioStore};
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
    events: StudioEventRuntime,
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
        events: StudioEventRuntime,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
    ) -> Self {
        Self {
            store,
            config_store,
            mcp_runtime,
            lsp_runtime,
            interactions,
            events,
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
        let task_phase = if mode == StudioMode::Task {
            self.store
                .find_active_task_run_for_session(&studio_session_id)
                .await
                .map_err(anyhow_error)?
                .map(|run| run.phase)
        } else {
            None
        };
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
        if let Some(effort) = route.reasoning_effort {
            builder = builder.with_reasoning_effort(effort);
        }
        let profile = CoreAgentProfile::local_workspace(workspace_root.clone())
            .with_workspace_instructions(workspace_instructions.clone());
        let subagent_context = if context.snapshot.identity.parent_id.is_some() {
            Some(SubagentContext {
                id: context.snapshot.identity.id.to_string(),
                // TaskCoordinator 的 durable owner boundary 是 Studio 逻辑树路径；
                // framework parent AgentId 只用于 runtime 协作授权，不能混入产品记录。
                parent_id: Some("/root".to_string()),
                agent_path: Some(format!("/root/{}", context.snapshot.identity.id)),
                role: context.snapshot.identity.role.to_string(),
                task: self
                    .resources
                    .get(&context.snapshot.identity.id)
                    .await
                    .map(|resource| resource.task_name)
                    .unwrap_or_else(|| context.snapshot.identity.role.to_string()),
                depth: context.snapshot.identity.depth,
            })
        } else {
            None
        };
        let kernel_builder = AgentKernel::builder(builder).with_profile(profile);
        let mut kernel = match subagent_context {
            Some(subagent) => kernel_builder.with_subagent_context(subagent).build().await,
            None => kernel_builder.build().await,
        };

        web_search.install(kernel.core_mut(), &config.web_search)?;

        if mode == StudioMode::Task {
            self.coordinator.install_tools(
                kernel.core_mut(),
                &studio_session_id,
                context.runtime.clone(),
                &context.snapshot,
            );
        }
        self.mcp_runtime
            .reconcile(crate::config::effective_mcp_servers(&config))
            .await?;
        self.lsp_runtime.reconcile_workspace(&workspace_root).await;
        self.mcp_runtime
            .acquire_turn_lease()
            .await?
            .install(kernel.core_mut())?;

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
        for tool in collaboration.tools() {
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
        let user_content = prompt_content(&context.input.message, &attachments);
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
        let request = TurnRequest::new(context.input.message)
            .with_turn_id(context.turn_id.to_string())
            .with_user_content(user_content)
            .with_materialized_attachments(materialized)
            .with_trace_attachments(trace_attachments)
            .with_workspace_instructions(workspace_instructions)
            .with_instruction_snapshot(instruction_snapshot);
        let emitter = interaction_emitter(self.events.clone(), studio_session_id.clone());
        let interaction_callback = self.interactions.callback(studio_session_id, emitter);
        let options = TurnOptions::default()
            .with_permission_mode(config.runtime.permission_mode)
            .with_interaction_callback(interaction_callback);
        let prepared = PreparedAgentTurn::new(kernel, request, options, policy);
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
    events: StudioEventRuntime,
    session_id: String,
) -> crate::studio::InteractionEmitter {
    Arc::new(move |interaction| {
        let events = events.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            events
                .emit_interaction(&session_id, crate::InteractionChangedEvent { interaction })
                .await?;
            Ok(())
        })
    })
}

fn turn_error(error: impl Into<String>) -> PureError {
    PureError::MemoryError(error.into())
}

fn anyhow_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

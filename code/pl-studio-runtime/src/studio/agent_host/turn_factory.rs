use std::path::Path;
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::time::Duration;

use crate::{ContentPart, ImageSource, MessageContent, PureError, Result};
use futures::FutureExt;
#[cfg(debug_assertions)]
use pl_core::TurnBudget;
use pl_core::instruction::{
    ExecutionInstructionProfile, InstructionAssembler, InstructionAssemblyRequest,
    InstructionSnapshot,
};
use pl_core::tool::{
    NamespaceDescriptor, ToolEntry, ToolRegistry, ToolSourceId, ToolSourceMetadata,
};
use pl_core::{
    AgentCollaborationTools, AgentIdentity, AgentTurnFactory, AgentTurnPreparationContext,
    CoreRuntimeProfile, PreparedAgentTurn, PreparedSessionRuntime, SubagentContext,
    ToolVisibilitySet, TurnEngineBuilder, TurnOptions, TurnRequest,
    load_workspace_instruction_documents, plan_web_search,
};

use crate::config::ConfigRuntime;
use crate::studio::runtime::SkillCatalogRuntime;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{InteractionService, StudioStore};
use crate::{McpRuntimeHandle, StudioMode};

use super::policy::{StudioPolicyContext, studio_execution_policy};
use super::resources::StudioAgentResources;
use super::workspace_resolver::AgentWorkspaceResolver;

/// 使用 Studio 配置、project/session 和产品工具准备一次 framework turn。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentTurnFactory {
    store: StudioStore,
    config_runtime: ConfigRuntime,
    mcp_runtime: McpRuntimeHandle,
    /// 与 MCP worker 共享的工具注册表；MCP 工具按 generation 发布于此。
    mcp_shared_tools: std::sync::Arc<ToolRegistry>,
    lsp_runtime: pl_lsp::LspRuntimeRegistry,
    interactions: InteractionService,
    coordinator: Arc<TaskCoordinator>,
    resources: StudioAgentResources,
    skills: SkillCatalogRuntime,
}

impl StudioAgentTurnFactory {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        store: StudioStore,
        config_runtime: ConfigRuntime,
        mcp_runtime: McpRuntimeHandle,
        mcp_shared_tools: std::sync::Arc<ToolRegistry>,
        lsp_runtime: pl_lsp::LspRuntimeRegistry,
        interactions: InteractionService,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
        skills: SkillCatalogRuntime,
    ) -> Self {
        Self {
            store,
            config_runtime,
            mcp_runtime,
            mcp_shared_tools,
            lsp_runtime,
            interactions,
            coordinator,
            resources,
            skills,
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
        let config = self.config_runtime.read()?.config;
        let skills = self.skills.read(&thread_record.project_id).await;
        let skill_catalog = skills.catalog_or_empty();
        let mode = thread_record.mode;
        let is_root = context.snapshot.identity.parent_id.is_none();
        // root 角色按 mode 派生；进程内 identity.role 只是投影，切换后允许短暂陈旧。
        let root_role = is_root.then(|| mode.root_role());
        let active_task_run = if mode == StudioMode::Task {
            self.store
                .find_active_task_run_for_root_thread(&thread_record.root_thread_id)
                .await
                .map_err(anyhow_error)?
        } else {
            None
        };
        let task_phase = active_task_run.as_ref().map(|run| run.kind());
        #[cfg(debug_assertions)]
        let task_driver_budget = debug_task_driver_budget_fixture()?;
        if let Some(run) = active_task_run.as_ref() {
            ensure_task_accepts_turn(run)?;
        }
        if mode == StudioMode::Task
            && context.snapshot.identity.parent_id.is_some()
            && matches!(
                context.snapshot.identity.role.as_str(),
                "executor" | "reviewer"
            )
            && active_task_run.is_none()
        {
            return Err(turn_error(
                "Task executor or reviewer has no active TaskRun",
            ));
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
        let workspace_instructions = load_workspace_instruction_documents(
            &workspace_root,
            &workspace_root,
            config.instructions.project_doc_max_bytes,
            &config.instructions.project_doc_fallback_filenames,
        )
        .map_err(anyhow_error)?
        .content();
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
                    let handoff =
                        crate::studio::task_coordinator::TaskExecutorHandoff::from_context_section(
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
                .mark_executor_turn_started(
                    context.snapshot.identity.id.as_str(),
                    context.turn_id.as_str(),
                    context.input.budget_action,
                )
                .await
                .map_err(anyhow_error)?;
            Some(section)
        } else {
            None
        };
        let input_message = context.input.payload.message.clone();
        let model_role = match root_role {
            Some(role) => role.id(),
            None => context.snapshot.identity.role.clone(),
        };
        let route = config.models.resolve(&model_role)?;
        let web_search = plan_web_search(&config.models, &route, &config.web_search)?;
        let mut builder = TurnEngineBuilder::from_route(&route)?
            .with_tool_capabilities(config.runtime.tool_capabilities.clone())
            .with_skills_config(config.skills.clone())
            .with_skill_catalog(skill_catalog.clone())
            .with_lsp_runtime(self.lsp_runtime.clone());
        if config.runtime.tool_capabilities.mcp {
            builder = builder.with_shared_tool_registry(self.mcp_shared_tools.clone());
        }
        let profile = CoreRuntimeProfile::local_agent_workspace(workspace.clone())
            .with_workspace_instructions(workspace_instructions.clone());
        let task_name = self
            .resources
            .get(&context.snapshot.identity.id)
            .await
            .map(|resource| resource.task_name)
            .unwrap_or_else(|| {
                root_role
                    .map(|role| role.key().to_string())
                    .unwrap_or_else(|| context.snapshot.identity.role.to_string())
            });
        let subagent_context = runtime_subagent_context(&context.snapshot.identity, task_name);
        let mut engine = builder.with_runtime_profile(profile).build();
        if let Some(subagent) = subagent_context {
            engine = engine.with_subagent_context(subagent);
        }
        engine.register_profile_tools().await;

        web_search.install(&mut engine, &config.web_search)?;

        if mode == StudioMode::Task {
            self.coordinator.install_tools(
                &mut engine,
                &thread_record.root_thread_id,
                context.runtime.clone(),
                &context.snapshot,
                active_task_run.as_ref(),
            );
        }
        let active_mcp_servers = self.mcp_runtime.available_server_names().await;
        let mcp_health = self.mcp_runtime.health_snapshot().await?;
        let active_lsp_servers = self
            .lsp_runtime
            .active_server_names_for_workspace(&workspace_root)
            .await;

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
        // 所有 agent（含 Task planner）共享同一套协作基础能力。send_message 统一
        // 作为 parent→direct-child 调度原语；子代理向主代理的报告改由 durable
        // 阶段提交 + read_agent_submissions 主动查询承载。
        let collaboration_source = ToolSourceId::collaboration();
        let collaboration_entries = collaboration
            .tools()
            .into_iter()
            .map(|tool| {
                ToolEntry::from_arc(
                    tool,
                    ToolSourceMetadata::new(collaboration_source.clone()).with_namespace(
                        NamespaceDescriptor::new(
                            "agents",
                            "Subagent discovery, messaging, waiting, and lifecycle tools.",
                        ),
                    ),
                )
            })
            .collect::<Vec<_>>();
        engine.register_source_tools(collaboration_source, collaboration_entries)?;

        let attachment_ids = context
            .input
            .payload
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
        let instruction_snapshot = instruction_snapshot(StudioInstructionContext {
            config: &config,
            model: &route.model,
            mode,
            role: model_role.as_str(),
            is_root,
            workspace_root: &workspace_root,
            workspace_instructions: &workspace_instructions,
            skill_catalog: &skill_catalog,
            subagent_constraint: context
                .input
                .payload
                .metadata
                .get("subagentConstraint")
                .and_then(serde_json::Value::as_str),
        })?;
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut request = TurnRequest::new(input_message)
            .with_turn_id(context.turn_id.to_string())
            .with_user_content(user_content)
            .with_materialized_attachments(materialized)
            .with_trace_attachments(trace_attachments)
            .with_workspace_instructions(workspace_instructions)
            .with_instruction_snapshot(instruction_snapshot);
        #[cfg(debug_assertions)]
        if let Some(fixture) = task_driver_budget
            && mode == StudioMode::Task
            && context.snapshot.identity.parent_id.is_some()
            && context.snapshot.identity.role.as_str() == crate::config::StudioRole::Executor.key()
            && context.input.budget_action == pl_core::MailboxBudgetAction::Preserve
        {
            request = request.with_budget(TurnBudget::new(fixture.executor_wall_clock_ms));
        }
        let emitter = interaction_emitter(
            context.runtime,
            thread_id.clone(),
            context.snapshot.identity.id.to_string(),
        );
        let interaction_callback = self.interactions.callback(thread_id, emitter);
        let prompt_cache_namespace = context.snapshot.identity.id.to_string();
        let prompt_role = root_role
            .map(|role| role.key().to_string())
            .unwrap_or_else(|| context.snapshot.identity.role.to_string());
        let prompt_scope = format!("{}:{prompt_role}", mode.label());
        // Turn 冻结工具诊断在本 turn 的 prompt snapshot 重建前不可得；host 读取
        // session prompt metadata 中该 scope 的当前 slot（即最近一次冻结的
        // lease 代数与 deferred catalog 指纹）作为 runtime 诊断投影。
        let tool_diagnostics = context
            .session
            .prompt_metadata()
            .slots
            .get(&prompt_scope)
            .map(|prompt| (prompt.registry_revision, prompt.tool_catalog_hash.clone()));
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut options = studio_turn_options(
            TurnOptions::default()
                .with_permission_mode(config.runtime.permission_mode)
                .with_prompt_cache_namespace(prompt_cache_namespace)
                .with_prompt_scope(prompt_scope)
                .with_interaction_callback(interaction_callback),
        );
        if is_root
            && task_phase == Some(crate::studio::task_coordinator::TaskRunStateKind::DesignUpdating)
        {
            let run = active_task_run
                .as_ref()
                .ok_or_else(|| turn_error("designUpdating turn has no active TaskRun"))?;
            options = options.with_tool_completion_callback(
                self.coordinator.design_tool_completion_callback(
                    run.id.clone(),
                    context.turn_id.to_string(),
                    workspace_root.clone(),
                ),
            );
        }
        #[cfg(debug_assertions)]
        if let Some(fixture) = task_driver_budget
            && mode == StudioMode::Task
            && context.snapshot.identity.parent_id.is_some()
            && context.snapshot.identity.role.as_str() == crate::config::StudioRole::Executor.key()
        {
            options = options.with_debug_context_compaction_timeout(Duration::from_millis(
                fixture.compaction_timeout_ms,
            ));
        }
        let mut session_runtime = PreparedSessionRuntime::new(route.model.slug.clone())
            .with_mcp_servers(active_mcp_servers)
            .with_mcp_health(mcp_health)
            .with_lsp(active_lsp_servers)
            .with_tool_diagnostics(
                tool_diagnostics
                    .as_ref()
                    .and_then(|(revision, _)| *revision),
                tool_diagnostics.and_then(|(_, catalog)| catalog),
            );
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
            .payload
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

fn studio_turn_options(options: TurnOptions) -> TurnOptions {
    options.with_user_input_end_turn()
}

#[cfg(debug_assertions)]
const TASK_DRIVER_EXECUTOR_WALL_CLOCK_ENV: &str = "PURE_STUDIO_TASK_DRIVER_EXECUTOR_WALL_CLOCK_MS";
#[cfg(debug_assertions)]
const TASK_DRIVER_COMPACTION_TIMEOUT_ENV: &str = "PURE_STUDIO_TASK_DRIVER_COMPACTION_TIMEOUT_MS";

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DebugTaskDriverBudgetFixture {
    executor_wall_clock_ms: u64,
    compaction_timeout_ms: u64,
}

#[cfg(debug_assertions)]
fn debug_task_driver_budget_fixture() -> Result<Option<DebugTaskDriverBudgetFixture>> {
    parse_debug_task_driver_budget_fixture(
        std::env::var(TASK_DRIVER_EXECUTOR_WALL_CLOCK_ENV)
            .ok()
            .as_deref(),
        std::env::var(TASK_DRIVER_COMPACTION_TIMEOUT_ENV)
            .ok()
            .as_deref(),
    )
}

#[cfg(debug_assertions)]
fn parse_debug_task_driver_budget_fixture(
    wall_clock_ms: Option<&str>,
    compaction_timeout_ms: Option<&str>,
) -> Result<Option<DebugTaskDriverBudgetFixture>> {
    let (Some(wall_clock_ms), Some(compaction_timeout_ms)) = (wall_clock_ms, compaction_timeout_ms)
    else {
        if wall_clock_ms.is_none() && compaction_timeout_ms.is_none() {
            return Ok(None);
        }
        return Err(turn_error(format!(
            "{TASK_DRIVER_EXECUTOR_WALL_CLOCK_ENV} and {TASK_DRIVER_COMPACTION_TIMEOUT_ENV} must be set together"
        )));
    };
    let executor_wall_clock_ms = wall_clock_ms.parse::<u64>().map_err(|error| {
        turn_error(format!(
            "invalid {TASK_DRIVER_EXECUTOR_WALL_CLOCK_ENV}: {error}"
        ))
    })?;
    let compaction_timeout_ms = compaction_timeout_ms.parse::<u64>().map_err(|error| {
        turn_error(format!(
            "invalid {TASK_DRIVER_COMPACTION_TIMEOUT_ENV}: {error}"
        ))
    })?;
    if compaction_timeout_ms == 0 {
        return Err(turn_error(format!(
            "{TASK_DRIVER_COMPACTION_TIMEOUT_ENV} must be greater than zero"
        )));
    }
    Ok(Some(DebugTaskDriverBudgetFixture {
        executor_wall_clock_ms,
        compaction_timeout_ms,
    }))
}

struct StudioInstructionContext<'a> {
    config: &'a crate::config::StudioConfig,
    model: &'a pl_model::ModelInfo,
    mode: StudioMode,
    role: &'a str,
    is_root: bool,
    workspace_root: &'a Path,
    workspace_instructions: &'a str,
    skill_catalog: &'a pl_core::skill::SkillCatalog,
    subagent_constraint: Option<&'a str>,
}

fn instruction_snapshot(context: StudioInstructionContext<'_>) -> Result<InstructionSnapshot> {
    InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&context.config.instructions),
        skills: Some(&context.config.skills),
        skill_catalog: Some(context.skill_catalog),
        execution_profile: Some(ExecutionInstructionProfile {
            label: context.mode.label(),
            instructions: context.mode.instructions_for(context.role, context.is_root),
        }),
        model: context.model,
        workspace_root: context.workspace_root,
        current_dir: context.workspace_root,
        workspace_instructions: Some(context.workspace_instructions),
        subagent_constraint: context.subagent_constraint,
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
        async move {
            let emitted_at = interaction.updated_at;
            runtime
                .record_thread_facts(
                    pl_core::ThreadId::new(agent_path.clone())?,
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
        }
        .boxed()
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

fn ensure_task_accepts_turn(run: &crate::studio::task_coordinator::TaskRun) -> Result<()> {
    if run.is_stop_requested() {
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
        DesignProgress, FinalizedDesign, StoppingState, TaskContext, TaskRun, TaskRunState,
        TaskStopOrigin, TaskStopReason, TaskStopRequest,
    };

    #[test]
    fn stop_requested_task_rejects_every_new_turn() {
        let run = stopped_run();
        assert!(ensure_task_accepts_turn(&run).is_err());
    }

    #[test]
    fn root_role_is_derived_from_mode_regardless_of_stale_identity() {
        let cases = [
            (StudioMode::Simple, "executor"),
            (StudioMode::Task, "planner"),
        ];
        for (mode, expected) in cases {
            assert_eq!(mode.root_role().key(), expected);
            assert_eq!(
                mode.instructions_for("stale-role", true),
                mode.instructions_for(mode.root_role().key(), true),
                "{} root instructions must depend only on mode",
                mode.label()
            );
        }
    }

    #[test]
    fn every_studio_turn_uses_durable_user_input_boundary() {
        let cases = [
            (StudioMode::Simple, crate::config::StudioRole::Executor),
            (StudioMode::Task, crate::config::StudioRole::Planner),
            (StudioMode::Task, crate::config::StudioRole::Executor),
        ];

        for (mode, role) in cases {
            let options = studio_turn_options(TurnOptions::default());
            assert_eq!(
                options.user_input_mode,
                pl_core::UserInputMode::EmitAndEndTurn,
                "{} {} turn must end after persisting user input",
                mode.label(),
                role.key()
            );
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_task_driver_budget_requires_a_complete_typed_pair() {
        assert_eq!(
            parse_debug_task_driver_budget_fixture(None, None).unwrap(),
            None
        );
        assert!(parse_debug_task_driver_budget_fixture(Some("0"), None).is_err());
        assert!(parse_debug_task_driver_budget_fixture(None, Some("250")).is_err());
        assert!(parse_debug_task_driver_budget_fixture(Some("0"), Some("0")).is_err());
        assert_eq!(
            parse_debug_task_driver_budget_fixture(Some("0"), Some("250")).unwrap(),
            Some(DebugTaskDriverBudgetFixture {
                executor_wall_clock_ms: 0,
                compaction_timeout_ms: 250,
            })
        );
    }

    fn stopped_run() -> TaskRun {
        TaskRun {
            context: TaskContext {
                id: "task-run".to_string(),
                project_id: "project".to_string(),
                root_thread_id: "session".to_string(),
                plan: "plan".to_string(),
                workspace_root: "C:/workspace".to_string(),
            },
            state: TaskRunState::Stopping(StoppingState::new(
                DesignProgress::from_finalized(FinalizedDesign {
                    summary: "design complete".to_string(),
                }),
                7,
                TaskStopRequest {
                    origin: TaskStopOrigin::UserRequest,
                    reason: TaskStopReason::new("stop").unwrap(),
                    requested_at: 1,
                },
            )),
            revision: 1,
            created_at: 1,
            updated_at: 1,
        }
    }
}

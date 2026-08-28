use std::path::Path;
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::time::Duration;

use crate::{AttachmentModality, ContentPart, MessageContent, PureError, Result};
use futures::FutureExt;
#[cfg(debug_assertions)]
use pl_core::TurnBudget;
use pl_core::instruction::{
    ExecutionInstructionProfile, InstructionAssembler, InstructionAssemblyRequest,
    InstructionSnapshot,
};
use pl_core::{
    AgentCollaborationTools, AgentIdentity, AgentTurnFactory, AgentTurnPreparationContext,
    AttachmentRuntime, BeforeModelStepHook, CoreRuntimeProfile, PreparedAgentTurn,
    PreparedSessionRuntime, SubagentContext, ToolGroupId, TurnEngineBuilder, TurnOptions,
    TurnRequest, load_workspace_instruction_documents, plan_web_search,
};

use crate::config::ConfigRuntime;
use crate::studio::product_event_bus::ProductEventBus;
use crate::studio::records::ThreadRecord;
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
    product_events: ProductEventBus,
    config_runtime: ConfigRuntime,
    mcp_runtime: McpRuntimeHandle,
    tool_manager: pl_core::ToolManager,
    lsp_runtime: pl_lsp::LspRuntimeRegistry,
    interactions: InteractionService,
    coordinator: Arc<TaskCoordinator>,
    resources: StudioAgentResources,
    skills: SkillCatalogRuntime,
    ssh_manager: Arc<pl_core::remote::SshManager>,
}

impl StudioAgentTurnFactory {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        store: StudioStore,
        product_events: ProductEventBus,
        config_runtime: ConfigRuntime,
        mcp_runtime: McpRuntimeHandle,
        tool_manager: pl_core::ToolManager,
        lsp_runtime: pl_lsp::LspRuntimeRegistry,
        interactions: InteractionService,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
        skills: SkillCatalogRuntime,
        ssh_manager: Arc<pl_core::remote::SshManager>,
    ) -> Self {
        Self {
            store,
            product_events,
            config_runtime,
            mcp_runtime,
            tool_manager,
            lsp_runtime,
            interactions,
            coordinator,
            resources,
            skills,
            ssh_manager,
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
        // 内存优先读取目录事实：注册与 mode 变更的落库是异步跟随的。
        let thread_record = match self.product_events.thread_snapshot(&thread_id) {
            Some(thread) => ThreadRecord::from_directory_thread(thread),
            None => self
                .store
                .read_thread(&thread_id)
                .await
                .map_err(anyhow_error)?
                .ok_or_else(|| turn_error("selected Studio Thread not found"))?,
        };
        let project = match self
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == thread_record.project_id)
        {
            Some(project) => project,
            None => self
                .store
                .read_project(&thread_record.project_id)
                .await
                .map_err(anyhow_error)?
                .ok_or_else(|| turn_error("selected Studio project not found"))?,
        };
        let config = self.config_runtime.read()?.config;
        let mode = thread_record.mode;
        let is_root = context.snapshot.identity.parent_id.is_none();
        // root 角色按 mode 派生；进程内 identity.role 只是投影，切换后允许短暂陈旧。
        let root_role = is_root.then(|| mode.root_role());
        let active_task = if mode == StudioMode::Task {
            self.coordinator
                .task_runtime()
                .aggregate(&thread_record.root_thread_id)
                .await
                .filter(|aggregate| !aggregate.facts.run.kind().is_terminal())
        } else {
            None
        };
        let active_task_run = active_task.as_ref().map(|aggregate| &aggregate.facts.run);
        let task_phase = active_task_run.map(|run| run.kind());
        #[cfg(debug_assertions)]
        let task_driver_budget = debug_task_driver_budget_fixture()?;
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

        let workspace = AgentWorkspaceResolver::new()
            .resolve(
                &context.snapshot.identity,
                &thread_record,
                &project,
                active_task.as_ref().map(|aggregate| &aggregate.facts),
            )
            .await
            .map_err(anyhow_error)?;
        let workspace_root = workspace.root().to_path_buf();
        let remote_host = match &project.ssh_server_id {
            Some(server_id) => Some(
                self.ssh_manager
                    .open_workspace_host(server_id, workspace_root.to_string_lossy().into_owned())
                    .await
                    .map_err(|error| turn_error(error.to_string()))?,
            ),
            None => None,
        };
        if let Some(remote_host) = &remote_host {
            self.lsp_runtime
                .apply_user_servers(&config.lsp.servers)
                .await
                .map_err(|error| {
                    turn_error(format!("invalid [lsp.servers] configuration: {error}"))
                })?;
            self.lsp_runtime
                .reconcile_workspace_membership_with_host(
                    &workspace_root,
                    Arc::new(remote_host.clone()),
                )
                .await;
            if self
                .lsp_runtime
                .active_server_names_for_workspace(&workspace_root)
                .await
                .is_empty()
            {
                self.lsp_runtime.probe_lsp_server(&workspace_root).await;
            }
        }
        let skill_catalog = if let Some(remote_host) = &remote_host {
            let registry = pl_core::skill::SkillRegistry::new();
            let remote_provider = Arc::new(pl_core::remote::RemoteSkillProvider::new(Arc::new(
                remote_host.files.clone(),
            ))?);
            let _remote_registration = registry.register(remote_provider)?;
            let mut local_sources = Vec::new();
            if let Ok(user_dir) = pl_core::skill::resolve_user_skills_dir(&config.skills) {
                local_sources.push(pl_core::skill::SkillDirectorySource::new(
                    user_dir,
                    pl_core::skill::SkillSourceKind::User,
                ));
            }
            if let Some(system_dir) = self.skills.system_skills_dir() {
                local_sources.push(pl_core::skill::SkillDirectorySource::new(
                    system_dir,
                    pl_core::skill::SkillSourceKind::System,
                ));
            }
            let _local_registration = if local_sources.is_empty() {
                None
            } else {
                Some(registry.register(Arc::new(
                    pl_core::skill::FileSystemSkillProvider::from_directories(
                        "remote-local-skills",
                        local_sources,
                    )?,
                ))?)
            };
            Some(Arc::new(
                registry
                    .discover(pl_core::skill::SkillProviderRequest {
                        workspace_root: workspace_root.clone(),
                        config: config.skills.clone(),
                        system_dir: None,
                        cancellation: context.cancellation_token.clone(),
                    })
                    .await?,
            ))
        } else {
            self.skills
                .discover_with_cancellation(
                    &thread_record.project_id,
                    &workspace_root,
                    &config.skills,
                    context.cancellation_token.clone(),
                )
                .await
                .map_err(anyhow_error)?
                .catalog_for_turn()
        };
        let mut turn_skills_config = config.skills.clone();
        if skill_catalog.is_none() {
            turn_skills_config.enabled = false;
        }
        let skill_catalog = skill_catalog.unwrap_or_else(|| {
            Arc::new(pl_core::skill::FrozenSkillCatalog::empty(
                workspace_root.join(&config.skills.project_dir),
            ))
        });
        let workspace_instructions = if let Some(remote_host) = &remote_host {
            pl_core::remote::load_remote_workspace_instructions(
                &remote_host.files,
                config.instructions.project_doc_max_bytes,
                &config.instructions.project_doc_fallback_filenames,
            )
            .await?
        } else {
            load_workspace_instruction_documents(
                &workspace_root,
                &workspace_root,
                config.instructions.project_doc_max_bytes,
                &config.instructions.project_doc_fallback_filenames,
            )
            .map_err(anyhow_error)?
            .content()
        };
        let executor_handoff = if mode == StudioMode::Task
            && context.snapshot.identity.parent_id.is_some()
            && context.snapshot.identity.role.as_str() == crate::config::StudioRole::Executor.key()
        {
            let run =
                active_task_run.ok_or_else(|| turn_error("Task executor has no active TaskRun"))?;
            let work_unit = active_task
                .as_ref()
                .and_then(|aggregate| {
                    aggregate.facts.work_units.iter().find(|unit| {
                        unit.executor_thread_id.as_deref()
                            == Some(context.snapshot.identity.id.as_str())
                    })
                })
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
                        work_unit,
                        context.snapshot.identity.id.as_str(),
                    )?;
                    Ok(section)
                });
            let section = match validated {
                Ok(section) => section,
                Err(error) => {
                    let message = error.to_string();
                    self.coordinator
                        .task_runtime()
                        .mark_executor_handoff_needs_attention(
                            context.snapshot.identity.id.as_str(),
                            &message,
                        )
                        .await
                        .map_err(anyhow_error)?;
                    return Err(turn_error(message));
                }
            };
            self.coordinator
                .task_runtime()
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
        let user_skill_load = if turn_skills_config.enabled {
            skill_catalog
                .load_user_invocations(
                    &input_message,
                    context.turn_id.as_str(),
                    context.cancellation_token.clone(),
                )
                .await
                .map_err(anyhow_error)?
        } else {
            pl_core::skill::SkillUserInvocationLoad::default()
        };
        let model_role = match root_role {
            Some(role) => role.id(),
            None => context.snapshot.identity.role.clone(),
        };
        let route = config.models.resolve(&model_role)?;
        let attachment_runtime = attachment_runtime(
            self.store.clone(),
            self.resources.clone(),
            thread_id.clone(),
        );
        let mcp_image_output =
            pl_core::McpImageOutputContext::for_model(&route.model, attachment_runtime.clone());
        let web_search = plan_web_search(&config.models, &route, &config.web_search)?;
        let agent_tools = self
            .resources
            .tool_set(&context.snapshot.identity.id, &self.tool_manager)
            .await;
        let exclusive_web_search =
            web_search.visibility == pl_core::ToolVisibilityConstraint::Exclusive;
        let refresh_mcp = config.runtime.tool_capabilities.mcp && !exclusive_web_search;
        let refresh_lsp = config.runtime.tool_capabilities.lsp && !exclusive_web_search;
        let mut builder = TurnEngineBuilder::from_route(&route)?
            .with_tool_capabilities(config.runtime.tool_capabilities.clone())
            .with_skills_config(turn_skills_config.clone())
            .with_skill_catalog(skill_catalog.clone())
            .with_lsp_runtime(self.lsp_runtime.clone())
            .with_agent_tool_set(agent_tools.clone());
        if refresh_mcp || refresh_lsp {
            let mcp_runtime = self.mcp_runtime.clone();
            let lsp_runtime = self.lsp_runtime.clone();
            let refresh_workspace = pl_core::ToolWorkspace::new(workspace.clone())
                .with_lsp_runtime(Some(lsp_runtime.clone()));
            let refresh_workspace_root = workspace_root.clone();
            let mcp_image_output = mcp_image_output.clone();
            builder = builder.with_before_model_step(BeforeModelStepHook::new(move |step| {
                let mcp_runtime = mcp_runtime.clone();
                let lsp_runtime = lsp_runtime.clone();
                let refresh_workspace = refresh_workspace.clone();
                let refresh_workspace_root = refresh_workspace_root.clone();
                let mcp_image_output = mcp_image_output.clone();
                async move {
                    let mut replacements = Vec::with_capacity(2);
                    if refresh_mcp {
                        let lease = mcp_runtime.acquire_turn_lease().await?;
                        replacements
                            .push((ToolGroupId::new("mcp"), lease.agent_tools(mcp_image_output)));
                    }
                    if refresh_lsp {
                        let available = !lsp_runtime
                            .active_server_names_for_workspace(&refresh_workspace_root)
                            .await
                            .is_empty();
                        replacements.push((
                            ToolGroupId::new("lsp"),
                            lsp_tool_group(available, lsp_runtime, refresh_workspace),
                        ));
                    }
                    step.agent_tools.install_batch(replacements)
                }
            }));
        }
        if !refresh_mcp {
            agent_tools.uninstall(&ToolGroupId::new("mcp"));
        }
        if !refresh_lsp {
            agent_tools.uninstall(&ToolGroupId::new("lsp"));
        }
        let profile = if remote_host.is_some() {
            CoreRuntimeProfile::minimal().with_agent_workspace(workspace.clone())
        } else {
            CoreRuntimeProfile::local_agent_workspace(workspace.clone())
        }
        .with_workspace_instructions(workspace_instructions.clone())
        .with_attachment_runtime(attachment_runtime.clone());
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
        if exclusive_web_search {
            for group in ["builtin", "skills", "lsp", "task", "collaboration", "mcp"] {
                engine.agent_tools().uninstall(&ToolGroupId::new(group));
            }
        } else if let Some(remote_host) = remote_host {
            let tool_workspace = pl_core::ToolWorkspace::new(workspace.clone())
                .with_lsp_runtime(Some(self.lsp_runtime.clone()));
            let files = Arc::new(remote_host.files);
            let commands = Arc::new(remote_host.commands);
            let git = Arc::new(remote_host.git);
            let mut additional_tools = if config.runtime.tool_capabilities.workspace_files {
                pl_core::remote::remote_workspace_mutation_tools(
                    files.clone(),
                    tool_workspace.clone(),
                )
            } else {
                Vec::new()
            };
            if let Some(tool) = pl_core::ViewImageTool::for_remote_model(
                tool_workspace,
                files.clone(),
                &route.model,
                attachment_runtime,
            ) {
                additional_tools.push(Arc::new(tool));
            }
            pl_core::BuiltinToolInstaller::host_provided(config.runtime.tool_capabilities.clone())
                .with_git_tools(
                    pl_core::GitWorkspaceConfig::local(workspace_root.clone())
                        .with_native_credentials(),
                    git,
                    Arc::new(pl_core::NoGitCredentialProvider),
                )
                .with_command_backend(commands)
                .with_workspace_file_backend(files)
                .with_additional_tools(additional_tools)
                .install_agent_workspace(
                    &mut engine,
                    workspace.clone(),
                    Some(workspace_instructions.clone()),
                )
                .await?;
        } else {
            engine.install_profile_tools().await?;
        }

        web_search.install(&mut engine, &config.web_search)?;
        if exclusive_web_search {
            engine
                .agent_tools()
                .uninstall(&ToolGroupId::new("programmatic_tool_calling"));
        } else {
            pl_core::reconcile_programmatic_tool_calling(engine.agent_tools(), &route)?;
        }

        if mode == StudioMode::Simple && !exclusive_web_search {
            engine.agent_tools().install(
                ToolGroupId::new("finalization"),
                vec![Arc::new(pl_core::PlanExitTool)],
            )?;
        } else {
            engine
                .agent_tools()
                .uninstall(&ToolGroupId::new("finalization"));
        }

        if mode == StudioMode::Task && !exclusive_web_search {
            self.coordinator.install_tools(
                &mut engine,
                &thread_record.root_thread_id,
                context.runtime.clone(),
                &context.snapshot,
                active_task_run,
            )?;
        } else {
            engine.agent_tools().uninstall(&ToolGroupId::new("task"));
        }
        let active_mcp_servers = self.mcp_runtime.available_server_names().await;
        let mcp_health = self.mcp_runtime.health_snapshot().await?;
        let active_lsp_servers = self
            .lsp_runtime
            .active_server_names_for_workspace(&workspace_root)
            .await;

        let policy =
            studio_execution_policy(&context.snapshot, StudioPolicyContext { mode, task_phase });
        let collaboration = AgentCollaborationTools::new(
            context.runtime.clone(),
            context.snapshot.identity.id.clone(),
            pl_core::AgentCollaborationToolConfig {
                policy: policy.collaboration.clone(),
                session_runtime: engine.tool_session_runtime(),
                workspace_root: workspace_root.clone(),
            },
        );
        // 所有 agent（含 Task planner）共享同一套协作基础能力。send_message 统一
        // 作为 parent→direct-child 调度原语；子代理向主代理的报告改由 durable
        // 阶段提交 + read_agent_submissions 主动查询承载。
        if exclusive_web_search {
            engine
                .agent_tools()
                .uninstall(&ToolGroupId::new("collaboration"));
        } else {
            engine
                .agent_tools()
                .install(ToolGroupId::new("collaboration"), collaboration.tools())?;
        }

        let attachment_ids = context
            .input
            .payload
            .attachments
            .iter()
            .map(|attachment| attachment.id.clone())
            .collect::<Vec<_>>();
        let attachments = self
            .resources
            .selected_thread_attachments(&thread_id, &attachment_ids)
            .await
            .map_err(anyhow_error)?;
        let stored_thread_attachments = attachments
            .iter()
            .map(crate::studio::store::attachment::thread_attachment)
            .collect::<Vec<_>>();
        if stored_thread_attachments != context.input.payload.attachments {
            return Err(anyhow_error(anyhow::anyhow!(
                "mailbox attachment manifest does not match the canonical attachment store"
            )));
        }
        let attachment_records = self.resources.thread_attachments(&thread_id).await;
        let mut materialized =
            crate::studio::store::attachment::materialize_attachment_records(attachment_records)
                .await
                .map_err(anyhow_error)?;
        let initial_remote_urls = self
            .resources
            .take_initial_remote_urls(&attachment_ids)
            .await;
        for attachment in &mut materialized {
            attachment.initial_remote_url =
                initial_remote_urls.get(&attachment.attachment_id).cloned();
        }
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
            skill_catalog: skill_catalog.snapshot(),
            skills_config: &turn_skills_config,
            subagent_constraint: context
                .input
                .payload
                .metadata
                .get("subagentConstraint")
                .and_then(pl_core::MailboxMetadataValue::as_str),
        })?;
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut request = TurnRequest::new(input_message)
            .with_turn_id(context.turn_id.to_string())
            .with_user_content(user_content)
            .with_materialized_attachments(materialized)
            .with_trace_attachments(trace_attachments)
            .with_skill_activations(user_skill_load.activations)
            .with_workspace_instructions(workspace_instructions)
            .with_instruction_snapshot(instruction_snapshot);
        if let Some(instruction) = user_skill_load.instruction {
            request = request.with_skill_invocation_instruction(instruction);
        }
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
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut options = studio_turn_options(
            TurnOptions::default()
                .with_permission_mode(config.runtime.permission_mode)
                .with_prompt_cache_namespace(prompt_cache_namespace)
                .with_prompt_scope(prompt_scope)
                .with_interaction_callback(interaction_callback),
        );
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
            .payload
            .metadata
            .get("historyPolicy")
            .and_then(pl_core::MailboxMetadataValue::as_str)
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
    skills_config: &'a pl_core::config::SkillsConfig,
    subagent_constraint: Option<&'a str>,
}

fn instruction_snapshot(context: StudioInstructionContext<'_>) -> Result<InstructionSnapshot> {
    InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&context.config.instructions),
        skills: Some(context.skills_config),
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
        return MessageContent::text(prompt.to_string());
    }
    let mut parts = Vec::new();
    if !prompt.is_empty() {
        parts.push(ContentPart::Text {
            text: prompt.to_string(),
        });
    }
    parts.extend(
        attachments
            .iter()
            .map(|attachment| ContentPart::Attachment {
                attachment_id: attachment.id.clone(),
                modality: match attachment.modality {
                    pl_protocol::studio::StudioAttachmentModality::Image => {
                        AttachmentModality::Image
                    }
                    pl_protocol::studio::StudioAttachmentModality::Video => {
                        AttachmentModality::Video
                    }
                    pl_protocol::studio::StudioAttachmentModality::File => AttachmentModality::File,
                },
                media_type: attachment.media_type.clone(),
                filename: attachment.filename.clone(),
            }),
    );
    MessageContent::new(parts)
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

fn attachment_runtime(
    store: StudioStore,
    resources: StudioAgentResources,
    thread_id: String,
) -> AttachmentRuntime {
    let writer_store = store.clone();
    let writer_resources = resources.clone();
    let writer_thread_id = thread_id.clone();
    AttachmentRuntime::new_batch(
        move |inputs| {
            let store = writer_store.clone();
            let resources = writer_resources.clone();
            let thread_id = writer_thread_id.clone();
            async move {
                let records = store
                    .persist_tool_image_records(&thread_id, inputs)
                    .await
                    .map_err(anyhow_error)?;
                let attachments = records
                    .iter()
                    .map(crate::studio::store::attachment::thread_attachment)
                    .collect();
                resources
                    .insert_thread_attachments(&thread_id, records)
                    .await;
                Ok(attachments)
            }
        },
        move |attachment_ids| {
            let resources = resources.clone();
            let thread_id = thread_id.clone();
            async move {
                let records = resources
                    .selected_thread_attachments(&thread_id, &attachment_ids)
                    .await
                    .map_err(anyhow_error)?;
                crate::studio::store::attachment::materialize_attachment_records(records)
                    .await
                    .map_err(anyhow_error)
            }
        },
    )
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

fn lsp_tool_group(
    available: bool,
    registry: pl_lsp::LspRuntimeRegistry,
    workspace: pl_core::ToolWorkspace,
) -> Vec<Arc<dyn pl_core::Tool>> {
    if available {
        pl_core::lsp_tools(registry, workspace)
    } else {
        Vec::new()
    }
}

fn anyhow_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn lsp_availability_replaces_the_agent_group_without_stale_tools() {
        let manager = pl_core::ToolManager::new();
        let tools = manager.agent_tool_set("lsp-switch", pl_core::GlobalToolInheritance::Isolated);
        let registry = pl_lsp::LspRuntimeRegistry::new();
        let workspace =
            pl_core::ToolWorkspace::new(pl_core::AgentWorkspace::local(std::env::temp_dir()));

        tools
            .install(
                ToolGroupId::new("lsp"),
                lsp_tool_group(true, registry.clone(), workspace.clone()),
            )
            .expect("publish available LSP tools");
        assert_eq!(
            tools.tool_names(),
            vec!["lsp_capabilities".to_string(), "lsp_query".to_string()]
        );

        tools
            .install(
                ToolGroupId::new("lsp"),
                lsp_tool_group(false, registry, workspace),
            )
            .expect("publish unavailable LSP generation");
        assert!(tools.tool_names().is_empty());
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
}

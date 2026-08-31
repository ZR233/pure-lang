use std::path::Path;
use std::sync::Arc;

use crate::{AttachmentModality, ContentPart, MessageContent, PureError, Result};
use futures::FutureExt;
use pl_core::WorkspaceInstructions;
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

use crate::McpRuntimeHandle;
use crate::StudioMode;
use crate::config::ConfigRuntime;
use crate::studio::product_event_bus::ProductEventBus;
use crate::studio::records::ThreadRecord;
use crate::studio::runtime::SkillCatalogRuntime;
use crate::studio::{InteractionService, StudioStore};

use super::policy::studio_execution_policy;
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
        let agent_profile_catalog = self.config_runtime.agent_profiles()?;
        for diagnostic in &agent_profile_catalog.diagnostics {
            tracing::warn!(
                path = %diagnostic.path.display(),
                message = %diagnostic.message,
                "ignored invalid Agent Profile"
            );
        }
        let available_agent_profiles = agent_profile_catalog.profiles;
        let mode = &thread_record.mode;
        let is_root = context.snapshot.identity.parent_id.is_none();
        // 所有模式的 root 统一使用 planner 路由；identity.role 只是可自愈投影。
        let root_role = is_root.then_some(crate::config::StudioRole::Planner);

        let workspace = AgentWorkspaceResolver::new()
            .resolve(&context.snapshot.identity, &thread_record, &project)
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
            let _mode_registration = if let Some(system_dir) = self.skills.system_skills_dir() {
                Some(registry.register(Arc::new(
                    pl_core::skill::FileSystemSkillProvider::from_directories(
                        pl_core::skill::BUILTIN_MODE_PROVIDER_ID,
                        vec![pl_core::skill::SkillDirectorySource::new(
                            // System assets are materialized with the
                            // stable `mode.*` names at the directory root.
                            system_dir,
                            pl_core::skill::SkillSourceKind::System,
                        )],
                    )?,
                ))?)
            } else {
                None
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
        let mode_snapshot = if is_root {
            Some(
                resolve_mode_snapshot(
                    &context.session,
                    &skill_catalog,
                    mode.label(),
                    context.cancellation_token.clone(),
                )
                .await?,
            )
        } else {
            None
        };
        let workspace_instruction_documents = if let Some(remote_host) = &remote_host {
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
        };
        let workspace_instructions = workspace_instruction_documents.content();
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
        let excluded_skill_names = user_skill_load
            .activations
            .iter()
            .map(|activation| activation.name.clone())
            .collect::<Vec<_>>();
        let frozen_agent_profile = if is_root {
            None
        } else {
            Some(
                context
                    .session
                    .agent_profile()
                    .cloned()
                    .ok_or_else(|| turn_error("child Agent session has no frozen Profile"))?,
            )
        };
        let model_role = root_role
            .map(|role| role.id())
            .unwrap_or_else(|| context.snapshot.identity.role.clone());
        let route = match &frozen_agent_profile {
            Some(profile) => resolve_frozen_profile_route(&config, profile)?,
            None => config.models.resolve(&model_role)?,
        };
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
        let assignment_name = self
            .resources
            .get(&context.snapshot.identity.id)
            .await
            .map(|resource| resource.assignment_name)
            .unwrap_or_else(|| {
                root_role
                    .map(|role| role.key().to_string())
                    .unwrap_or_else(|| context.snapshot.identity.role.to_string())
            });
        let subagent_context =
            runtime_subagent_context(&context.snapshot.identity, assignment_name);
        let mut engine = builder.with_runtime_profile(profile).build();
        if let Some(subagent) = subagent_context {
            engine = engine.with_subagent_context(subagent);
        }
        if exclusive_web_search {
            for group in [
                "builtin",
                "skills",
                "lsp",
                "workflow",
                "collaboration",
                "mcp",
            ] {
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

        engine
            .agent_tools()
            .uninstall(&ToolGroupId::new("finalization"));
        engine.agent_tools().uninstall(&ToolGroupId::new("task"));
        if let Some(mode_snapshot) = mode_snapshot.clone()
            && !exclusive_web_search
        {
            let working_set = engine.tool_session_runtime().working_set();
            engine.agent_tools().install(
                ToolGroupId::new("workflow"),
                vec![Arc::new(pl_core::WorkflowStateTool::new(
                    working_set,
                    mode_snapshot,
                ))],
            )?;
        } else {
            engine
                .agent_tools()
                .uninstall(&ToolGroupId::new("workflow"));
        }
        let active_mcp_servers = self.mcp_runtime.available_server_names().await;
        let mcp_health = self.mcp_runtime.health_snapshot().await?;
        let active_lsp_servers = self
            .lsp_runtime
            .active_server_names_for_workspace(&workspace_root)
            .await;

        let policy = studio_execution_policy(&context.snapshot, &available_agent_profiles);
        let collaboration = AgentCollaborationTools::new(
            context.runtime.clone(),
            context.snapshot.identity.id.clone(),
            pl_core::AgentCollaborationToolConfig {
                policy: policy.collaboration.clone(),
                session_runtime: engine.tool_session_runtime(),
                workspace_root: workspace_root.clone(),
                profiles: available_agent_profiles.clone(),
            },
        );
        // 所有 Agent Profile 共享同一套协作基础能力。send_message 统一
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
        let execution_instructions = if let Some(mode_snapshot) = &mode_snapshot {
            format!(
                "{}\n\n<preloaded_mode_skill name=\"{}\" revision=\"{}\" contentHash=\"{}\">\n{}\n</preloaded_mode_skill>",
                StudioMode::root_instructions(),
                mode_snapshot.mode_id,
                mode_snapshot.revision,
                mode_snapshot.content_hash,
                mode_snapshot.content,
            )
        } else {
            frozen_agent_profile
                .as_ref()
                .map(|profile| profile.system_instructions.clone())
                .ok_or_else(|| turn_error("child Agent session has no frozen Profile"))?
        };
        let instruction_snapshot = instruction_snapshot(StudioInstructionContext {
            config: &config,
            model: &route.model,
            execution_label: mode.label(),
            execution_instructions: &execution_instructions,
            workspace_root: &workspace_root,
            workspace_documents: Some(&workspace_instruction_documents),
            workspace_instructions: &workspace_instructions,
            skill_catalog: skill_catalog.snapshot(),
            skills_config: &turn_skills_config,
            skill_query: &input_message,
            excluded_skill_names: &excluded_skill_names,
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
        let options = studio_turn_options(
            TurnOptions::default()
                .with_permission_mode(config.runtime.permission_mode)
                .with_prompt_cache_namespace(prompt_cache_namespace)
                .with_prompt_scope(prompt_scope)
                .with_interaction_callback(interaction_callback),
        );
        let mut session_runtime = PreparedSessionRuntime::new(route.model.slug.clone())
            .with_mcp_servers(active_mcp_servers)
            .with_mcp_health(mcp_health)
            .with_lsp(active_lsp_servers);
        if let Some(context_window) = route.model.resolved_context_window() {
            session_runtime = session_runtime.with_context_window(context_window);
        }
        let prepared = PreparedAgentTurn::new(engine, request, options, policy)
            .with_session_runtime(session_runtime);
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

struct StudioInstructionContext<'a> {
    config: &'a crate::config::StudioConfig,
    model: &'a pl_model::ModelInfo,
    execution_label: &'a str,
    execution_instructions: &'a str,
    workspace_root: &'a Path,
    workspace_documents: Option<&'a WorkspaceInstructions>,
    workspace_instructions: &'a str,
    skill_catalog: &'a pl_core::skill::SkillCatalog,
    skills_config: &'a pl_core::config::SkillsConfig,
    skill_query: &'a str,
    excluded_skill_names: &'a [String],
    subagent_constraint: Option<&'a str>,
}

fn instruction_snapshot(context: StudioInstructionContext<'_>) -> Result<InstructionSnapshot> {
    InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&context.config.instructions),
        skills: Some(context.skills_config),
        skill_catalog: Some(context.skill_catalog),
        execution_profile: Some(ExecutionInstructionProfile {
            label: context.execution_label,
            instructions: context.execution_instructions,
        }),
        model: context.model,
        workspace_root: context.workspace_root,
        current_dir: context.workspace_root,
        workspace_documents: context.workspace_documents,
        workspace_instructions: Some(context.workspace_instructions),
        subagent_constraint: context.subagent_constraint,
        skill_suggestions: Some(pl_core::instruction::SkillSuggestionRequest {
            query: context.skill_query,
            excluded_names: context.excluded_skill_names,
        }),
    })
}

async fn resolve_mode_snapshot(
    session: &pl_core::AgentSession,
    catalog: &pl_core::skill::FrozenSkillCatalog,
    mode_id: &str,
    cancellation: tokio_util::sync::CancellationToken,
) -> Result<pl_protocol::ModeInstructionSnapshot> {
    if let Some(run) = session
        .workflow()
        .and_then(|state| state.current_run.as_ref())
        && run.lifecycle == pl_protocol::WorkflowRunLifecycle::Active
    {
        return Ok(run.mode.clone());
    }
    let metadata = catalog.find_mode(mode_id).ok_or_else(|| {
        turn_error(format!(
            "selected Mode Skill `{mode_id}` is unavailable; refresh Skills or choose an available mode"
        ))
    })?;
    let definition = catalog
        .load(
            mode_id,
            pl_core::skill::SkillLoadInvocation::Mode,
            cancellation,
        )
        .await?;
    let mode = metadata
        .mode
        .as_ref()
        .ok_or_else(|| turn_error(format!("selected Skill `{mode_id}` has no mode metadata")))?;
    Ok(pl_protocol::ModeInstructionSnapshot {
        mode_id: definition.summary.name,
        display_name: mode.display_name.clone(),
        source: skill_source_label(definition.summary.source).to_string(),
        provider_id: definition.summary.provider_id.as_str().to_string(),
        revision: definition.revision,
        content_hash: pl_core::canonical_content_hash(definition.content.as_bytes()),
        content: definition.content,
    })
}

fn skill_source_label(source: pl_core::skill::SkillSourceKind) -> &'static str {
    match source {
        pl_core::skill::SkillSourceKind::Project => "project",
        pl_core::skill::SkillSourceKind::User => "user",
        pl_core::skill::SkillSourceKind::System => "system",
        pl_core::skill::SkillSourceKind::External => "external",
    }
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

fn resolve_frozen_profile_route(
    config: &crate::config::StudioConfig,
    profile: &pl_protocol::AgentProfileSnapshot,
) -> Result<pl_core::ResolvedModelRoute> {
    let role = pl_core::AgentRoleId::new(profile.profile_id.clone())?;
    let mut models = config.models.clone();
    models.routes.insert(
        role.clone(),
        pl_core::ModelRouteConfig {
            provider: pl_core::ProviderId::new(profile.provider_id.clone())?,
            model: profile.model.clone(),
            effort: profile
                .effort
                .as_ref()
                .map(|effort| pl_core::ReasoningEffort::new(effort.clone())),
        },
    );
    models.resolve(&role)
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

    fn workflow_with_mode(
        mode: pl_protocol::ModeInstructionSnapshot,
        lifecycle: pl_protocol::WorkflowRunLifecycle,
    ) -> pl_protocol::WorkflowSessionState {
        pl_protocol::WorkflowSessionState {
            revision: 1,
            current_run: Some(pl_protocol::WorkflowRun {
                lineage_id: "lineage-1".to_string(),
                run_id: "run-1".to_string(),
                definition: pl_protocol::WorkflowDefinition::default(),
                definition_hash: "sha256:definition".to_string(),
                mode,
                lifecycle,
                current_stage_id: "working".to_string(),
                compiled_at: 1,
                updated_at: 1,
                history_tail: Vec::new(),
                archived_transition_count: 0,
                archived_transition_digest: String::new(),
            }),
            ..pl_protocol::WorkflowSessionState::default()
        }
    }

    #[test]
    fn every_mode_uses_the_unified_planner_root() {
        let instructions = StudioMode::root_instructions();
        for mode in [
            StudioMode::simple(),
            StudioMode::task(),
            StudioMode::new("mode.release").unwrap(),
        ] {
            assert!(!instructions.contains(mode.label()));
        }
    }

    #[test]
    fn every_studio_turn_uses_durable_user_input_boundary() {
        let cases = [
            (StudioMode::simple(), crate::config::StudioRole::Planner),
            (StudioMode::task(), crate::config::StudioRole::Planner),
            (StudioMode::task(), crate::config::StudioRole::Executor),
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

    #[tokio::test]
    async fn active_workflow_keeps_deleted_mode_snapshot_and_terminal_refreshes_latest_skill() {
        let root = tempfile::TempDir::new().unwrap();
        let workspace = tempfile::TempDir::new().unwrap();
        let skill_dir = root.path().join("skills/mode.release");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        let document = |body: &str| {
            format!(
                "---\nname: mode.release\ndescription: Release mode\ndisable-model-invocation: true\nuser-invocable: false\nmode:\n  display-name: Release\n  order: 30\n---\n\n{body}\n"
            )
        };
        std::fs::write(&skill_file, document("version one")).unwrap();
        let registry = pl_core::skill::SkillRegistry::new();
        let _registration = registry
            .register(Arc::new(
                pl_core::skill::FileSystemSkillProvider::from_directories(
                    "custom-mode-test",
                    vec![pl_core::skill::SkillDirectorySource::new(
                        root.path().join("skills"),
                        pl_core::skill::SkillSourceKind::User,
                    )],
                )
                .unwrap(),
            ))
            .unwrap();
        let request = || pl_core::skill::SkillProviderRequest {
            workspace_root: workspace.path().to_path_buf(),
            config: pl_core::config::SkillsConfig::default(),
            system_dir: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        };
        let first_catalog = registry.discover(request()).await.unwrap();
        let initial = resolve_mode_snapshot(
            &pl_core::AgentSession::default(),
            &first_catalog,
            "mode.release",
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(initial.content.contains("version one"));

        let mut session = pl_core::AgentSession::default();
        session.replace_workflow(Some(workflow_with_mode(
            initial.clone(),
            pl_protocol::WorkflowRunLifecycle::Active,
        )));
        std::fs::remove_file(&skill_file).unwrap();
        let empty = pl_core::skill::FrozenSkillCatalog::empty(workspace.path().join("skills"));
        let frozen = resolve_mode_snapshot(
            &session,
            &empty,
            "mode.release",
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(frozen, initial);

        session.replace_workflow(Some(workflow_with_mode(
            initial,
            pl_protocol::WorkflowRunLifecycle::Terminal,
        )));
        assert!(
            resolve_mode_snapshot(
                &session,
                &empty,
                "mode.release",
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .is_err()
        );

        std::fs::write(&skill_file, document("version two")).unwrap();
        let second_catalog = registry.discover(request()).await.unwrap();
        let refreshed = resolve_mode_snapshot(
            &session,
            &second_catalog,
            "mode.release",
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(refreshed.content.contains("version two"));
    }
}

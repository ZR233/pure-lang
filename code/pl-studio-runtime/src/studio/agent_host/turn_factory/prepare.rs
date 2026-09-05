//! `AgentTurnFactory::prepare_turn` 的编排实现。

use std::sync::Arc;

use pl_core::ExecutionEnvironment;
use pl_core::{
    AgentCollaborationTools, AgentIdentity, AgentTurnFactory, AgentTurnPreparationContext,
    BeforeModelStepHook, CoreRuntimeProfile, PreparedAgentTurn, PreparedSessionRuntime,
    SubagentContext, ToolGroupId, ToolInstallGroup, TurnEngineBuilder, TurnOptions, TurnRequest,
    load_workspace_instruction_documents, plan_web_searches,
};

use crate::studio::records::ThreadRecord;
use crate::{PureError, Result};

use super::super::policy::studio_execution_policy;
use super::super::workspace_resolver::AgentWorkspaceResolver;
use super::attachments::{attachment_runtime, prompt_content};
use super::errors::{anyhow_error, turn_error};
use super::factory::StudioAgentTurnFactory;
use super::instructions::{StudioInstructionContext, instruction_snapshot};
use super::interactions::interaction_emitter;
use super::routing::{resolve_frozen_profile_route, validate_thread_mode_model};
use super::tools::lsp_tool_group;

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
            .resolve(
                &context.snapshot.identity,
                &thread_record,
                &project,
                context.session.workspace_assignment(),
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
        let execution_environment = remote_host
            .as_ref()
            .map(|host| host.execution_environment.clone())
            .unwrap_or_else(ExecutionEnvironment::detect_local);
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
            let system_dir = self.skills.system_skills_dir();
            let (registry, _registrations) = self
                .skills
                .remote_workspace_registry(
                    &config.skills,
                    system_dir.as_deref(),
                    Arc::new(remote_host.files.clone()),
                )
                .map_err(|error| turn_error(format!("{error:#}")))?;
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
        let registered_mode = if is_root {
            Some(self.thread_modes.snapshot().mode(mode).ok_or_else(|| {
                turn_error(format!("selected Thread Mode `{mode}` is unavailable"))
            })?)
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
        validate_thread_mode_model(registered_mode.as_deref(), &route.model)?;
        let attachment_runtime = attachment_runtime(
            self.store.clone(),
            self.resources.clone(),
            thread_id.clone(),
        );
        let mcp_image_output =
            pl_core::McpImageOutputContext::for_model(&route.model, attachment_runtime.clone());
        let web_search = plan_web_searches(
            &config.models,
            &route,
            &config.web_search,
            config.deepseek_web_search.enabled,
        )?;
        let agent_tools = self
            .resources
            .tool_set(&context.snapshot.identity.id, &self.tool_manager)
            .await;
        let exclusive_web_search =
            web_search.visibility() == pl_core::ToolVisibilityConstraint::Exclusive;
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
                        replacements.push(ToolInstallGroup::deferred(
                            ToolGroupId::new("mcp"),
                            lease.agent_tools(mcp_image_output)?,
                        ));
                    }
                    if refresh_lsp {
                        let available = !lsp_runtime
                            .active_server_names_for_workspace(&refresh_workspace_root)
                            .await
                            .is_empty();
                        replacements.push(ToolInstallGroup::direct(
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
        .with_attachment_runtime(attachment_runtime.clone())
        .with_execution_environment(execution_environment.clone())
        .with_agent_session_plan(
            pl_core::AgentSessionPlanOptions::default()
                .with_submitted_plan_presentation(pl_core::MessagePresentation::Hidden),
        );
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
                "plan",
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
                additional_tools.push(tool.into());
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

        if let Some(registered_mode) = &registered_mode {
            engine
                .tool_session_runtime()
                .working_set()
                .set_thread_mode(Some(registered_mode.clone()));
        }
        if let Some(registered_mode) = registered_mode
            .as_ref()
            .filter(|mode| mode.workflow().is_some())
        {
            let working_set = engine.tool_session_runtime().working_set();
            engine.agent_tools().install(ToolInstallGroup::direct(
                ToolGroupId::new("workflow"),
                vec![
                    pl_core::WorkflowCurrentTool::new(working_set.clone(), registered_mode.clone())
                        .into(),
                    pl_core::WorkflowNextTool::new(working_set.clone(), registered_mode.clone())
                        .into(),
                    pl_core::WorkflowGraphTool::new(working_set.clone(), registered_mode.clone())
                        .into(),
                    pl_core::WorkflowHistoryTool::new(working_set.clone(), registered_mode.clone())
                        .into(),
                    pl_core::WorkflowTransitionTool::new(
                        working_set.clone(),
                        registered_mode.clone(),
                    )
                    .into(),
                    pl_core::WorkflowRestartTool::new(working_set, registered_mode.clone()).into(),
                ],
            ))?;
        } else {
            engine
                .agent_tools()
                .uninstall(&ToolGroupId::new("workflow"));
        }
        if is_root {
            engine.agent_tools().install(ToolInstallGroup::direct(
                ToolGroupId::new("completion"),
                vec![pl_core::CompleteTool.into()],
            ))?;
        } else {
            engine
                .agent_tools()
                .uninstall(&ToolGroupId::new("completion"));
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
            engine.agent_tools().install(ToolInstallGroup::direct(
                ToolGroupId::new("collaboration"),
                collaboration.tools(),
            ))?;
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
        let execution_instructions = if let Some(registered_mode) = &registered_mode {
            format!(
                "<preloaded_thread_mode_prompt modeId=\"{}\" graphRevision=\"{}\" graphHash=\"{}\">\n{}\n</preloaded_thread_mode_prompt>",
                registered_mode.descriptor().id,
                registered_mode.graph_revision(),
                registered_mode.graph_hash().unwrap_or("none"),
                registered_mode.prompt(),
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
            execution_environment: &execution_environment,
        })?;
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut request = TurnRequest::new(input_message)
            .with_turn_id(context.turn_id.to_string())
            .with_user_content(user_content)
            .with_user_presentation(context.input.payload.presentation)
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
        let mut prepared = PreparedAgentTurn::new(engine, request, options, policy)
            .with_session_runtime(session_runtime);
        if let Some(registered_mode) = &registered_mode
            && let Some(workflow) = pl_core::reconcile_workflow_for_turn(
                context.session.workflow().cloned(),
                registered_mode,
                context.turn_id.as_str(),
                crate::studio::unix_seconds(),
            )?
        {
            prepared = prepared.with_initial_workflow(workflow);
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

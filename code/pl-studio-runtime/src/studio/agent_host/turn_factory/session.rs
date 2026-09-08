//! Session creation is the sole Studio tool assembly entrypoint.

use std::sync::Arc;

use pl_core::session_runtime::{SessionBuildContext, SessionRuntimeBuilder};
use pl_core::{
    BeforeModelStepHook, BuiltinToolInstaller, ToolGroupId, ToolInstallGroup, ToolWorkspace,
};

use super::super::policy::studio_execution_policy;
use super::super::workspace_resolver::AgentWorkspaceResolver;
use super::StudioAgentTurnFactory;
use super::attachments::attachment_runtime;
use super::errors::{anyhow_error, turn_error};
use super::routing::resolve_frozen_profile_route;

impl StudioAgentTurnFactory {
    pub(super) async fn session_host_identity(
        &self,
        project: &crate::studio::records::ProjectRecord,
    ) -> crate::Result<String> {
        let server = match &project.ssh_server_id {
            Some(id) => Some(
                self.ssh_manager
                    .list_servers()
                    .await
                    .into_iter()
                    .find(|server| &server.id == id)
                    .ok_or_else(|| turn_error("session SSH server is unavailable"))?,
            ),
            None => None,
        };
        Ok(pl_core::canonical_json_hash(&serde_json::json!([
            project.id,
            project.path,
            server,
        ])))
    }

    pub(super) async fn build_session(
        &self,
        context: SessionBuildContext,
    ) -> crate::Result<SessionRuntimeBuilder> {
        let id = context.identity().id.clone();
        if self.product_events.thread_snapshot(id.as_str()).is_none() {
            let thread = self
                .store
                .read_thread(id.as_str())
                .await
                .map_err(anyhow_error)?
                .ok_or_else(|| turn_error("restored session has no product association"))?;
            let attachments = self
                .store
                .list_thread_attachments(id.as_str())
                .await
                .map_err(anyhow_error)?;
            self.resources
                .replace_thread_attachments(id.as_str(), attachments)
                .await;
            self.product_events
                .warm_thread_index(vec![pl_core::Thread::from(thread)]);
        }
        let thread = self
            .product_events
            .thread_snapshot(id.as_str())
            .map(crate::studio::records::ThreadRecord::from_directory_thread)
            .ok_or_else(|| turn_error("session Thread directory is not resident"))?;
        let project = self
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == thread.project_id)
            .ok_or_else(|| turn_error("session project is not resident"))?;
        let config = self.config_runtime.read()?.config;
        let host_identity = self.session_host_identity(&project).await?;
        let workspace = AgentWorkspaceResolver::new()
            .resolve(
                context.identity(),
                &thread,
                &project,
                context.session().workspace_assignment(),
            )
            .await
            .map_err(anyhow_error)?;
        let root = workspace.root().to_path_buf();
        let remote = match &project.ssh_server_id {
            Some(server) => Some(
                self.ssh_manager
                    .open_workspace_host(server, root.to_string_lossy().into_owned())
                    .await
                    .map_err(|error| turn_error(error.to_string()))?,
            ),
            None => None,
        };
        let is_root = context.identity().parent_id.is_none();
        let route = if is_root {
            config
                .models
                .resolve(&crate::config::StudioRole::Planner.id())?
        } else {
            resolve_frozen_profile_route(
                &config,
                context
                    .session()
                    .agent_profile()
                    .ok_or_else(|| turn_error("child session has no frozen Profile"))?,
            )?
        };
        let searches = pl_core::plan_web_searches(
            &config.models,
            &route,
            &config.web_search,
            config.deepseek_web_search.enabled,
        )?;
        let environment = remote
            .as_ref()
            .map(|host| host.execution_environment.clone())
            .unwrap_or_else(pl_core::ExecutionEnvironment::detect_local);
        let binding = pl_core::session_runtime::SessionWorkspaceBinding::new(
            workspace.clone(),
            environment.clone(),
            host_identity,
        );
        let workspace =
            ToolWorkspace::new(workspace).with_lsp_runtime(Some(self.lsp_runtime.clone()));
        let session = context.tool_session_runtime();
        let capabilities = config.runtime.tool_capabilities.clone();
        let mut groups = Vec::new();
        let mut builder =
            SessionRuntimeBuilder::new(self.tool_manager.clone()).with_workspace(binding);
        if is_root {
            builder = builder.with_event_source(
                "studio-thread",
                super::thread_events::ThreadTitleEvents {
                    events: self.product_events.clone(),
                    thread_id: id.to_string(),
                },
            );
        }
        let all_capabilities = pl_core::ToolCapabilityConfig {
            git: true,
            ..Default::default()
        };
        let builtin = if let Some(host) = &remote {
            let files = Arc::new(host.files.clone());
            let extra =
                pl_core::remote::remote_workspace_mutation_tools(files.clone(), workspace.clone());
            BuiltinToolInstaller::host_provided(all_capabilities.clone())
                .with_git_tools(
                    pl_core::GitWorkspaceConfig::local(root.clone()).with_native_credentials(),
                    Arc::new(host.git.clone()),
                    Arc::new(pl_core::NoGitCredentialProvider),
                )
                .with_command_backend(Arc::new(host.commands.clone()))
                .with_workspace_file_backend(files)
                .with_additional_tools(extra)
                .build_catalog(workspace.clone(), session.clone(), environment)
        } else {
            let backend = pl_core::tool::LocalCommandBackend::new(root.clone())
                .with_execution_environment(environment.clone());
            #[cfg(target_os = "linux")]
            let backend = backend.with_worker_executable(
                self.local_worker
                    .get_or_try_init(crate::studio::runtime::local_worker)
                    .await?
                    .clone(),
            );
            BuiltinToolInstaller::from_capabilities(all_capabilities)
                .with_command_backend(Arc::new(backend))
                .build_catalog(workspace.clone(), session.clone(), environment)
        };
        groups.extend(builtin.groups(&capabilities, searches.visibility()));
        let profiles = self.config_runtime.agent_profiles()?.profiles;
        let policy = studio_execution_policy(context.identity(), &profiles);
        let collaboration = pl_core::AgentCollaborationTools::new(
            context.runtime().clone(),
            id.clone(),
            pl_core::AgentCollaborationToolConfig {
                policy: policy.collaboration,
                session_runtime: session.clone(),
                workspace_root: root.clone(),
                profiles,
            },
        );
        if let Some(source) = collaboration.event_source() {
            builder = builder.with_event_source("agents", source);
        }
        groups.push(ToolInstallGroup::direct(
            ToolGroupId::new("collaboration"),
            collaboration.tools(),
        ));
        if is_root {
            groups.push(ToolInstallGroup::direct(
                ToolGroupId::new("completion"),
                vec![pl_core::CompleteTool.into()],
            ));
        }
        groups.push(searches.build_group(&config.web_search, session.clone())?);
        let attachments = attachment_runtime(self, id.to_string());
        let mcp = self.mcp_runtime.clone();
        let lsp = self.lsp_runtime.clone();
        let config_runtime = self.config_runtime.clone();
        let identity = context.identity().clone();
        let agent_runtime = context.runtime().clone();
        let refresh = BeforeModelStepHook::new(move |step| {
            let mcp = mcp.clone();
            let lsp = lsp.clone();
            let workspace = workspace.clone();
            let attachments = attachments.clone();
            let root = root.clone();
            let remote = remote.clone();
            let session = session.clone();
            let mut route = route.clone();
            route.model = step.model.clone();
            route.endpoint = step.endpoint.clone();
            let config_runtime = config_runtime.clone();
            let identity = identity.clone();
            let agent_runtime = agent_runtime.clone();
            let builtin = builtin.clone();
            async move {
                let config = config_runtime.read()?.config;
                let capabilities = &config.runtime.tool_capabilities;
                let searches = pl_core::plan_web_searches(
                    &config.models,
                    &route,
                    &config.web_search,
                    config.deepseek_web_search.enabled,
                )?;
                let exclusive =
                    searches.visibility() == pl_core::ToolVisibilityConstraint::Exclusive;
                let mut replacements = builtin.groups(capabilities, searches.visibility());
                replacements.push(searches.build_group(&config.web_search, session.clone())?);
                let profiles = config_runtime.agent_profiles()?.profiles;
                let policy = studio_execution_policy(&identity, &profiles);
                let collaboration = pl_core::AgentCollaborationTools::new(
                    agent_runtime,
                    identity.id.clone(),
                    pl_core::AgentCollaborationToolConfig {
                        policy: policy.collaboration,
                        session_runtime: session.clone(),
                        workspace_root: root.clone(),
                        profiles,
                    },
                );
                replacements.push(ToolInstallGroup::direct(
                    ToolGroupId::new("collaboration"),
                    if exclusive {
                        Vec::new()
                    } else {
                        collaboration.tools()
                    },
                ));
                if capabilities.mcp && !exclusive {
                    let lease = mcp.acquire_turn_lease().await?;
                    replacements.push(ToolInstallGroup::deferred(
                        ToolGroupId::new("mcp"),
                        lease.agent_tools(pl_core::McpImageOutputContext::for_model(
                            &step.model,
                            attachments.clone(),
                        ))?,
                    ));
                } else {
                    replacements.push(ToolInstallGroup::deferred(
                        ToolGroupId::new("mcp"),
                        Vec::new(),
                    ));
                }
                if capabilities.lsp && !exclusive {
                    let available = !lsp
                        .active_server_names_for_workspace(&root)
                        .await
                        .is_empty();
                    replacements.push(ToolInstallGroup::direct(
                        ToolGroupId::new("lsp"),
                        super::tools::lsp_tool_group(available, lsp, workspace.clone()),
                    ));
                } else {
                    replacements.push(ToolInstallGroup::direct(
                        ToolGroupId::new("lsp"),
                        Vec::new(),
                    ));
                }
                {
                    if capabilities.skills && !exclusive {
                        let tools = step.skill_catalog.map_or_else(Vec::new, |catalog| {
                            pl_core::tool::skill_tools_from_catalog(
                                catalog,
                                workspace.clone(),
                                pl_core::tool::SkillToolMode::ProjectWritable,
                            )
                        });
                        replacements
                            .push(ToolInstallGroup::direct(ToolGroupId::new("skills"), tools));
                    } else {
                        replacements.push(ToolInstallGroup::direct(
                            ToolGroupId::new("skills"),
                            Vec::new(),
                        ));
                    }
                    replacements.push(workflow_group(step.thread_mode, session.working_set()));
                    let image = if exclusive {
                        None
                    } else if let Some(host) = remote {
                        pl_core::ViewImageTool::for_remote_model(
                            workspace,
                            Arc::new(host.files),
                            &step.model,
                            attachments,
                        )
                    } else {
                        pl_core::ViewImageTool::for_model(workspace, &step.model, attachments)
                    };
                    replacements.push(ToolInstallGroup::direct(
                        ToolGroupId::new("view_image"),
                        image.into_iter().map(Into::into).collect(),
                    ));
                }
                if !replacements.is_empty() {
                    step.agent_tools.install_batch(replacements)?;
                }
                Ok(())
            }
        });
        builder = builder.with_refresh(refresh);
        for group in groups {
            builder = builder.with_tools(group);
        }
        Ok(builder)
    }
}

fn workflow_group(
    mode: Option<Arc<pl_core::RegisteredThreadMode>>,
    working: pl_core::TurnWorkingSetHandle,
) -> ToolInstallGroup {
    let tools = mode
        .filter(|mode| mode.workflow().is_some())
        .map_or_else(Vec::new, |mode| {
            vec![
                pl_core::WorkflowCurrentTool::new(working.clone(), mode.clone()).into(),
                pl_core::WorkflowNextTool::new(working.clone(), mode.clone()).into(),
                pl_core::WorkflowGraphTool::new(working.clone(), mode.clone()).into(),
                pl_core::WorkflowHistoryTool::new(working.clone(), mode.clone()).into(),
                pl_core::WorkflowTransitionTool::new(working.clone(), mode.clone()).into(),
                pl_core::WorkflowRestartTool::new(working, mode).into(),
            ]
        });
    ToolInstallGroup::direct(ToolGroupId::new("workflow"), tools)
}

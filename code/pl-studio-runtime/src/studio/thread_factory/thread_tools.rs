//! Shared physical tool assembly for new roots, children and cold restoration.
use super::StudioThreadFactory;
use super::errors::resource_error;
use crate::resource_store::FileResourceStore;
use crate::studio::ProjectRecord;
use crate::thread_assembler::{
    StudioCommandBinding, StudioGitBinding, StudioThreadTools, StudioWorkspaceTools,
    ThreadAssemblyError,
};
use pl_tool::workspace::ToolWorkspace;
use std::sync::Arc;

pub(super) struct ThreadToolAssembly<'a> {
    pub thread_id: &'a str,
    pub cancellation: &'a tokio_util::sync::CancellationToken,
    pub config: &'a crate::config::StudioConfig,
    pub route: &'a pl_model::config::ResolvedModelRoute,
    pub project: &'a ProjectRecord,
    pub root_thread_id: &'a str,
    pub workspace: ToolWorkspace,
    pub store: FileResourceStore,
}

pub(super) struct PreparedThreadTools {
    pub catalog: Option<Arc<pl_tool::skill::FrozenSkillCatalog>>,
    pub visibility: crate::search::ToolVisibilityConstraint,
    pub tools: StudioThreadTools,
    pub hosted: Vec<pl_model::runtime::HostedTool>,
    pub environment: pl_tool::environment::ExecutionEnvironment,
    pub remote: Option<pl_tool::remote::RemoteWorkspaceHost>,
}

pub(super) enum CatalogPreparation {
    Initial,
    Refresh {
        commands: Option<crate::thread_assembler::StudioCommandProcesses>,
        remote: Option<Box<pl_tool::remote::RemoteWorkspaceHost>>,
        policy_key: String,
        policies: std::collections::BTreeMap<
            String,
            pl_core::tool::execution_policy::ExecutionPolicyHandle,
        >,
    },
}

impl StudioThreadFactory {
    pub(super) async fn prepare_thread_tools(
        &self,
        assembly: ThreadToolAssembly<'_>,
    ) -> Result<PreparedThreadTools, ThreadAssemblyError> {
        let binding = super::tool_bindings::ToolBinding {
            incarnation: std::sync::Arc::new(()),
            target_invalidated: false,
            initial_skill_prompt: String::new(),
            catalog: None,
            policy_key: approval_policy_key(assembly.config, assembly.route),
            policies: Default::default(),
            project: assembly.project.clone(),
            workspace: assembly.workspace.clone(),
            commands: None,
            remote: None,
            ssh_profile: match &assembly.project.ssh_server_id {
                Some(id) => self
                    .services
                    .ssh_manager
                    .list_servers()
                    .await
                    .into_iter()
                    .find(|profile| &profile.id == id),
                None => None,
            },
        };
        let id = assembly.thread_id.to_owned();
        let tools = self
            .prepare_thread_tools_with(assembly, CatalogPreparation::Initial)
            .await?;
        let binding = super::tool_bindings::ToolBinding {
            remote: tools.remote.clone(),
            catalog: tools.catalog.clone(),
            initial_skill_prompt: tools
                .catalog
                .as_ref()
                .map(|catalog| pl_tool::skill::build_skills_prompt_from_catalog(catalog.snapshot()))
                .unwrap_or_default(),
            policies: tools.tools.approval_policies.clone(),
            commands: tools
                .tools
                .command_processes
                .as_ref()
                .map(|commands| commands.downgrade()),
            ..binding
        };
        self.bindings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, binding);
        Ok(tools)
    }

    pub(super) async fn prepare_thread_tools_with(
        &self,
        assembly: ThreadToolAssembly<'_>,
        preparation: CatalogPreparation,
    ) -> Result<PreparedThreadTools, ThreadAssemblyError> {
        let ThreadToolAssembly {
            thread_id,
            cancellation,
            config,
            route,
            project,
            root_thread_id,
            workspace,
            store,
        } = assembly;
        let mut search = crate::search::plan_web_searches(
            &config.models,
            route,
            &config.web_search,
            config.deepseek_web_search.enabled,
        )?
        .build_thread(&config.web_search)?;
        if search.visibility != crate::search::ToolVisibilityConstraint::Exclusive {
            search
                .hosted
                .extend(crate::programmatic::hosted_tool(route));
        }
        let root = workspace.root().to_owned();
        let remote = match &preparation {
            CatalogPreparation::Refresh { remote, .. } => remote.as_deref().cloned(),
            CatalogPreparation::Initial => match &project.ssh_server_id {
                Some(server) => Some(
                    self.services
                        .ssh_manager
                        .open_workspace_host(server, root.to_string_lossy().into_owned())
                        .await
                        .map_err(|error| resource_error("open child remote workspace", error))?,
                ),
                None => None,
            },
        };
        if cancellation.is_cancelled() {
            return Err(pl_core::thread::ThreadError::Cancelled.into());
        }
        let environment = remote
            .as_ref()
            .map(|host| host.execution_environment.clone())
            .unwrap_or_else(pl_tool::environment::ExecutionEnvironment::detect_local);
        if search.visibility == crate::search::ToolVisibilityConstraint::Exclusive {
            return Ok(PreparedThreadTools {
                catalog: None,
                visibility: search.visibility,
                tools: StudioThreadTools::selected(store, search.tools),
                hosted: search.hosted,
                environment,
                remote,
            });
        }
        let binding = match &remote {
            Some(host) => StudioCommandBinding::Remote {
                host: host.clone(),
                workspace: workspace.clone(),
            },
            None => StudioCommandBinding::Local {
                workspace: workspace.clone(),
                environment: environment.clone(),
                access: match config.runtime.permission_mode {
                    crate::approval::PermissionMode::FullAccess
                        if workspace.workspace().boundary().allows_host_paths() =>
                    {
                        pl_tool::exec::CommandAccess::HostGranted
                    }
                    crate::approval::PermissionMode::AutoReview
                    | crate::approval::PermissionMode::RequestApproval
                        if workspace.workspace().boundary().allows_host_paths() =>
                    {
                        pl_tool::exec::CommandAccess::HostApproval
                    }
                    crate::approval::PermissionMode::FullAccess
                    | crate::approval::PermissionMode::AutoReview
                    | crate::approval::PermissionMode::RequestApproval => {
                        pl_tool::exec::CommandAccess::WorkspaceOnly
                    }
                },
            },
        };
        let commands = match &preparation {
            CatalogPreparation::Initial => None,
            CatalogPreparation::Refresh { commands, .. } => commands.clone(),
        };
        let mut tools = StudioThreadTools::with_command_processes(
            StudioWorkspaceTools {
                binding,
                store,
                capabilities: config.runtime.tool_capabilities.clone(),
            },
            commands,
        )
        .await?;
        if config.runtime.tool_capabilities.ask_user {
            tools = tools.with_tools(crate::plan_tool::plan_registrations(Default::default())?);
        }
        if config.runtime.tool_capabilities.git {
            let git =
                pl_tool::git::GitWorkspaceConfig::local(root.clone()).with_native_credentials();
            let credentials = Arc::new(pl_tool::git::NoGitCredentialProvider);
            tools = match &remote {
                Some(host) => tools.with_git(StudioGitBinding {
                    config: git,
                    backend: Arc::new(host.git.clone()),
                    credentials,
                    kinds: pl_tool::git::GitToolKind::all().to_vec(),
                    authorization: workspace.authorization(),
                })?,
                None => tools.with_git(StudioGitBinding {
                    config: git,
                    backend: Arc::new(pl_tool::execution::LocalExecutionBackend),
                    credentials,
                    kinds: pl_tool::git::GitToolKind::all().to_vec(),
                    authorization: workspace.authorization(),
                })?,
            };
        }
        if config.runtime.tool_capabilities.mcp {
            tools = tools.with_mcp(
                self.services
                    .mcp_runtime
                    .acquire_turn_lease()
                    .await
                    .map_err(|error| resource_error("acquire child MCP services", error))?,
            )?;
        }
        let mut skill_catalog = None;
        if config.runtime.tool_capabilities.skills && config.skills.enabled {
            let catalog = if matches!(preparation, CatalogPreparation::Refresh { .. }) {
                Some(
                    self.services
                        .skills
                        .freeze_thread_catalog(
                            &root,
                            &config.skills,
                            cancellation.clone(),
                            remote.as_ref().map(|host| Arc::new(host.files.clone())),
                        )
                        .await
                        .map_err(|error| resource_error("refresh Thread skill catalog", error))?,
                )
            } else {
                match &remote {
                    Some(host) => {
                        self.services
                            .skills
                            .discover_remote(
                                &project.id,
                                &root,
                                &config.skills,
                                cancellation.clone(),
                                Arc::new(host.files.clone()),
                            )
                            .await
                    }
                    None => {
                        self.services
                            .skills
                            .discover_with_cancellation(
                                &project.id,
                                &root,
                                &config.skills,
                                cancellation.clone(),
                            )
                            .await
                    }
                }
                .map_err(|error| resource_error("discover child skills", error))?
                .catalog_for_turn()
            };
            if let Some(catalog) = catalog {
                skill_catalog = Some(catalog.clone());
                tools = tools.with_skills(catalog.clone())?;
                if remote.is_none() {
                    tools = tools.with_local_skill_management(catalog, workspace.clone())?;
                }
            }
        }
        let reviewer =
            if config.runtime.permission_mode == crate::approval::PermissionMode::AutoReview {
                let performance = self.services.model_performance.clone();
                let root_thread_id = root_thread_id.to_owned();
                let thread_id = thread_id.to_owned();
                let usage = Arc::new(move |billing: pl_protocol::InferenceBillingRecord| {
                    performance.record_auxiliary_inference(&root_thread_id, &thread_id, &billing)
                });
                Some(crate::tool_review::reviewer(route, usage)?)
            } else {
                None
            };
        let approval = crate::thread_assembler::StudioApprovalOptions {
            mode: config.runtime.permission_mode,
            host: if remote.is_some() {
                crate::thread_assembler::ApprovalHost::Remote
            } else {
                crate::thread_assembler::ApprovalHost::Local
            },
            workspace: workspace.clone(),
            reviewer,
        };
        if config.runtime.tool_capabilities.lsp {
            let paths = match &remote {
                Some(host) => pl_tool::lsp::LspPathBinding::Remote {
                    workspace,
                    files: Arc::new(host.files.clone()),
                },
                None => pl_tool::lsp::LspPathBinding::Local(workspace),
            };
            tools = tools.with_lsp(self.services.lsp_runtime.clone(), paths)?;
        }
        let policies = match &preparation {
            CatalogPreparation::Refresh {
                policy_key,
                policies,
                ..
            } if *policy_key == approval_policy_key(config, route) => policies.clone(),
            CatalogPreparation::Initial | CatalogPreparation::Refresh { .. } => Default::default(),
        };
        Ok(PreparedThreadTools {
            catalog: skill_catalog,
            visibility: search.visibility,
            tools: tools
                .with_tools(search.tools)
                .with_approval(approval, &policies),
            hosted: search.hosted,
            environment,
            remote,
        })
    }
}

pub(super) fn approval_policy_key(
    config: &crate::config::StudioConfig,
    route: &pl_model::config::ResolvedModelRoute,
) -> String {
    crate::hash::canonical_json_hash(&serde_json::json!({
        "mode": config.runtime.permission_mode,
        "reviewer": (config.runtime.permission_mode == crate::approval::PermissionMode::AutoReview).then(|| serde_json::json!({"provider": route.provider_id, "endpoint": route.endpoint, "model": route.model, "effort": route.effort, "pricing": route.pricing_mode})),
    }))
}

//! Creation-time standard catalog; the resulting registrations belong exclusively to core Thread.
use super::{StudioThreadSpec, ThreadAssemblyError};
use crate::resource_store::FileResourceStore;
use pl_core::{
    context::{OpaquePayload, ResourceAccess},
    tool::opaque::Registration,
};
use pl_tool::{
    exec::{CommandAccess, ThreadExecTool},
    task_control::{TaskControlKind, TaskControlTool},
    thread_catalog::ThreadBuiltin,
};
use std::sync::Arc;

/// Explicit physical command binding chosen from the current product configuration.
#[derive(Debug)]
pub enum StudioCommandBinding {
    Local {
        workspace: pl_tool::workspace::ToolWorkspace,
        environment: pl_tool::environment::ExecutionEnvironment,
        access: CommandAccess,
    },
    Remote {
        host: pl_tool::remote::RemoteWorkspaceHost,
        workspace: pl_tool::workspace::ToolWorkspace,
    },
}

/// Physical command controllers remain unique per Thread across catalog refreshes.
#[derive(Debug, Clone)]
pub(crate) enum StudioCommandProcesses {
    Local(Arc<pl_tool::command::CommandProcessManager<pl_tool::command::LocalCommandBackend>>),
    Remote(Arc<pl_tool::command::CommandProcessManager<pl_tool::remote::RemoteCommandBackend>>),
}
#[derive(Debug, Clone)]
pub(crate) enum WeakCommandProcesses {
    Local(
        std::sync::Weak<
            pl_tool::command::CommandProcessManager<pl_tool::command::LocalCommandBackend>,
        >,
    ),
    Remote(
        std::sync::Weak<
            pl_tool::command::CommandProcessManager<pl_tool::remote::RemoteCommandBackend>,
        >,
    ),
}
impl StudioCommandProcesses {
    pub(crate) fn downgrade(&self) -> WeakCommandProcesses {
        match self {
            Self::Local(manager) => WeakCommandProcesses::Local(Arc::downgrade(manager)),
            Self::Remote(manager) => WeakCommandProcesses::Remote(Arc::downgrade(manager)),
        }
    }
}
impl WeakCommandProcesses {
    pub(crate) fn upgrade(&self) -> Option<StudioCommandProcesses> {
        match self {
            Self::Local(manager) => manager.upgrade().map(StudioCommandProcesses::Local),
            Self::Remote(manager) => manager.upgrade().map(StudioCommandProcesses::Remote),
        }
    }
}

/// Explicit Git resources and allowed operation kinds selected by the product host.
#[derive(Debug)]
pub struct StudioGitBinding<B, P> {
    pub config: pl_tool::git::GitWorkspaceConfig,
    pub backend: Arc<B>,
    pub credentials: Arc<P>,
    pub kinds: Vec<pl_tool::git::GitToolKind>,
    pub authorization: pl_core::tool::opaque::ToolAuthorization,
}

/// Physical resources and capability policy resolved from one Studio configuration snapshot.
#[derive(Debug)]
pub struct StudioWorkspaceTools {
    pub binding: StudioCommandBinding,
    pub store: FileResourceStore,
    pub capabilities: pl_tool::workspace::ToolCapabilityConfig,
}

/// Ready-to-transfer instances and their matching resource reader.
#[derive(Debug)]
pub struct StudioThreadTools {
    pub(super) registrations: Vec<Registration>,
    pub(crate) command_processes: Option<StudioCommandProcesses>,
    pub(crate) approval_policies:
        std::collections::BTreeMap<String, pl_core::tool::execution_policy::ExecutionPolicyHandle>,
    pub(super) store: FileResourceStore,
    pub(super) capabilities: pl_tool::workspace::ToolCapabilityConfig,
}

impl StudioThreadTools {
    /// Transfers an explicitly selected catalog, without constructing standard capabilities.
    /// Optional capability builders remain disabled for this restricted catalog.
    pub fn selected(store: FileResourceStore, registrations: Vec<Registration>) -> Self {
        Self {
            command_processes: None,
            approval_policies: Default::default(),
            store,
            registrations,
            capabilities: pl_tool::workspace::ToolCapabilityConfig {
                exec: false,
                workspace_files: false,
                skills: false,
                mcp: false,
                lsp: false,
                ask_user: false,
                git: false,
            },
        }
    }

    /// Constructs fresh standard tools for one Thread, using shared storage only for immutable bytes.
    ///
    /// # Errors
    /// Returns declaration encoding or registration failure before publishing a Thread.
    pub async fn standard(options: StudioWorkspaceTools) -> Result<Self, ThreadAssemblyError> {
        Self::with_command_processes(options, None).await
    }

    pub(crate) async fn with_command_processes(
        options: StudioWorkspaceTools,
        mut commands: Option<StudioCommandProcesses>,
    ) -> Result<Self, ThreadAssemblyError> {
        let StudioWorkspaceTools {
            binding,
            store,
            capabilities,
        } = options;
        let mut registrations = Vec::new();
        match binding {
            StudioCommandBinding::Local {
                workspace,
                environment,
                access,
            } => {
                if capabilities.exec {
                    let processes = match &commands {
                        Some(StudioCommandProcesses::Local(processes)) => processes.clone(),
                        Some(StudioCommandProcesses::Remote(_)) => {
                            return Err(ThreadAssemblyError::Identity(
                                "physical command binding changed; reactivate this Thread".into(),
                            ));
                        }
                        None => {
                            let backend = pl_tool::command::LocalCommandBackend::new(
                                workspace.root().to_path_buf(),
                            )
                            .with_execution_environment(environment);
                            #[cfg(target_os = "linux")]
                            let backend = backend.with_worker_executable(
                                crate::worker_assets::local_worker().await?,
                            );
                            Arc::new(pl_tool::command::CommandProcessManager::new(Arc::new(
                                backend,
                            )))
                        }
                    };
                    commands = Some(StudioCommandProcesses::Local(processes.clone()));
                    registrations.extend(
                        ThreadExecTool::from_process_manager(
                            processes,
                            Arc::new(store.local_command_archive()),
                            access,
                        )
                        .registrations(
                            declaration(ThreadBuiltin::Exec)?,
                            declaration(ThreadBuiltin::WriteStdin)?,
                        )?,
                    );
                }
                if capabilities.workspace_files {
                    let files = Arc::new(
                        pl_tool::workspace_file::LocalWorkspaceFileBackend::confined(
                            workspace.clone(),
                        )
                        .await?,
                    );
                    registrations.extend(local_mutation_tools(workspace.clone())?);
                    registrations.extend(file_tools(
                        files,
                        workspace,
                        Arc::new(super::media::MediaHost(store.clone())),
                    )?);
                }
            }
            StudioCommandBinding::Remote { host, workspace } => {
                if capabilities.exec {
                    let archive = store.remote_command_archive(Arc::new(host.files.clone()));
                    let processes = match &commands {
                        Some(StudioCommandProcesses::Remote(processes)) => processes.clone(),
                        Some(StudioCommandProcesses::Local(_)) => {
                            return Err(ThreadAssemblyError::Identity(
                                "physical command binding changed; reactivate this Thread".into(),
                            ));
                        }
                        None => Arc::new(pl_tool::command::CommandProcessManager::new(Arc::new(
                            host.commands.clone(),
                        ))),
                    };
                    commands = Some(StudioCommandProcesses::Remote(processes.clone()));
                    registrations.extend(
                        ThreadExecTool::from_process_manager(
                            processes,
                            Arc::new(archive),
                            CommandAccess::WorkspaceOnly,
                        )
                        .registrations(
                            declaration(ThreadBuiltin::Exec)?,
                            declaration(ThreadBuiltin::WriteStdin)?,
                        )?,
                    );
                }
                if capabilities.workspace_files {
                    for kind in [
                        pl_tool::remote::RemoteMutationKind::CreateDirectory,
                        pl_tool::remote::RemoteMutationKind::Delete,
                        pl_tool::remote::RemoteMutationKind::Copy,
                        pl_tool::remote::RemoteMutationKind::Move,
                    ] {
                        let tool = pl_tool::remote::RemoteWorkspaceMutationTool::new(
                            kind,
                            Arc::new(host.files.clone()),
                            workspace.clone(),
                        );
                        let spec = tool.declaration();
                        registrations.push(
                            Registration::new(
                                spec.name().into(),
                                pl_model::runtime::thread_tool_declaration(&spec)?,
                                tool,
                            )?
                            .with_authorization(workspace.authorization()),
                        );
                    }
                    let files = pl_tool::workspace_file::WorkspacePolicyBackend::new(
                        Arc::new(host.files.clone()),
                        workspace.clone(),
                    );
                    registrations.extend(file_tools(
                        Arc::new(files),
                        workspace,
                        Arc::new(super::media::MediaHost(store.clone())),
                    )?);
                }
            }
        }
        if capabilities.ask_user {
            registrations.push(pl_tool::ask_user::registration(declaration(
                ThreadBuiltin::AskUser,
            )?)?);
        }
        registrations.extend([
            pl_tool::complete::registration(declaration(ThreadBuiltin::Complete)?)?,
            pl_tool::todo::registration(declaration(ThreadBuiltin::Todo)?)?,
            pl_tool::discovery::registration(declaration(ThreadBuiltin::Discover)?)?,
        ]);
        registrations.push(
            pl_tool::task_control::ListTasksTool
                .registration(declaration(ThreadBuiltin::ListTasks)?)?,
        );
        registrations.push(Registration::new(
            "sleep".into(),
            declaration(ThreadBuiltin::Sleep)?,
            pl_tool::session::SleepTool,
        )?);
        for kind in [
            TaskControlKind::Wait,
            TaskControlKind::Query,
            TaskControlKind::Cancel,
        ] {
            registrations.push(
                TaskControlTool::new(kind).registration(declaration(ThreadBuiltin::Task(kind))?)?,
            );
        }
        for kind in pl_tool::session_note::SessionNoteToolKind::all() {
            registrations.push(pl_tool::session_note::registration(
                *kind,
                declaration(ThreadBuiltin::Note(*kind))?,
            )?);
        }
        Ok(Self {
            command_processes: commands,
            approval_policies: Default::default(),
            registrations,
            store,
            capabilities,
        })
    }

    /// Installs fresh Git tool instances over shared execution and credential services.
    ///
    /// # Errors
    /// Returns model declaration or registry errors.
    pub fn with_git<
        B: pl_tool::execution::ExecutionBackend + 'static,
        P: pl_tool::git::GitCredentialProvider + 'static,
    >(
        mut self,
        binding: StudioGitBinding<B, P>,
    ) -> Result<Self, ThreadAssemblyError> {
        if !self.capabilities.git {
            return Ok(self);
        }
        for kind in binding.kinds {
            let declaration = pl_model::runtime::thread_tool_declaration(&kind.to_spec())?;
            let tool = pl_tool::git::GitTool::new(
                kind,
                binding.config.clone(),
                binding.backend.clone(),
                binding.credentials.clone(),
            );
            self.registrations
                .push(tool.registration(declaration, binding.authorization.clone())?);
        }
        Ok(self)
    }

    /// Installs independent search with explicit service parameters and core-provided frozen context.
    ///
    /// # Errors
    /// Returns model declaration encoding or registry failures.
    pub fn with_web_search(
        mut self,
        client: pl_tool::search::WebSearchClient,
        options: pl_tool::search::ThreadSearchOptions,
    ) -> Result<Self, ThreadAssemblyError> {
        let declaration = pl_model::runtime::thread_tool_declaration(
            &pl_tool::search::ThreadWebSearchTool::declaration(),
        )?;
        self.registrations.push(
            pl_tool::search::ThreadWebSearchTool::new(client, options).registration(declaration)?,
        );
        Ok(self)
    }

    /// Installs skill discovery and read snapshots over a frozen provider directory.
    ///
    /// # Errors
    /// Returns declaration encoding or registry errors.
    pub fn with_skills(
        mut self,
        catalog: Arc<pl_tool::skill::FrozenSkillCatalog>,
    ) -> Result<Self, ThreadAssemblyError> {
        if !self.capabilities.skills {
            return Ok(self);
        }
        for kind in [
            pl_tool::skill::ThreadSkillKind::List,
            pl_tool::skill::ThreadSkillKind::View,
        ] {
            let declaration = pl_model::runtime::thread_tool_declaration(&kind.declaration())?;
            let tool = pl_tool::skill::ThreadSkillTool::new(catalog.clone(), kind);
            self.registrations.push(tool.registration(declaration)?);
        }
        Ok(self)
    }

    /// Adds project Skill mutations only for an explicitly local writable binding.
    ///
    /// # Errors
    /// Returns declaration or tool registration failures.
    pub fn with_local_skill_management(
        mut self,
        catalog: Arc<pl_tool::skill::FrozenSkillCatalog>,
        workspace: pl_tool::workspace::ToolWorkspace,
    ) -> Result<Self, ThreadAssemblyError> {
        if !self.capabilities.skills {
            return Ok(self);
        }
        let declaration = pl_model::runtime::thread_tool_declaration(
            &pl_tool::skill::ThreadSkillManageTool::declaration(),
        )?;
        self.registrations.push(
            pl_tool::skill::ThreadSkillManageTool::new(catalog, workspace)
                .registration(declaration)?,
        );
        Ok(self)
    }

    /// Binds language-service tools to the selected physical workspace without rebuilding them each Turn.
    ///
    /// # Errors
    /// Returns declaration encoding or registration errors.
    pub fn with_lsp(
        mut self,
        registry: pl_lsp::runtime::LspRuntimeRegistry,
        paths: pl_tool::lsp::LspPathBinding,
    ) -> Result<Self, ThreadAssemblyError> {
        if !self.capabilities.lsp {
            return Ok(self);
        }
        for kind in [
            pl_tool::lsp::LspToolKind::Capabilities,
            pl_tool::lsp::LspToolKind::Query,
        ] {
            let declaration = pl_model::runtime::thread_tool_declaration(&kind.declaration())?;
            let tool = pl_tool::lsp::ThreadLspTool::new(registry.clone(), paths.clone(), kind);
            self.registrations.push(tool.registration(declaration)?);
        }
        Ok(self)
    }

    /// Adds host-selected file, MCP, LSP or product tools before transferring the complete catalog.
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Registration>) -> Self {
        self.registrations.extend(tools);
        self
    }

    pub(crate) fn into_registrations(self) -> Vec<Registration> {
        self.registrations
    }

    /// Transfers this catalog and reader into the one assembly specification.
    pub fn install(self, mut spec: StudioThreadSpec) -> StudioThreadSpec {
        spec.tools.extend(self.registrations);
        spec.resources = ResourceAccess::new(self.store);
        spec
    }
}

fn declaration(tool: ThreadBuiltin) -> Result<OpaquePayload, ThreadAssemblyError> {
    Ok(pl_model::runtime::thread_tool_declaration(
        &tool.declaration(),
    )?)
}

fn local_mutation_tools(
    workspace: pl_tool::workspace::ToolWorkspace,
) -> Result<Vec<Registration>, ThreadAssemblyError> {
    use pl_tool::file::{CopyPathTool, CreateDirectoryTool, DeletePathTool, MovePathTool};
    let authority = workspace.authorization();
    Ok(vec![
        Registration::new(
            "create_directory".into(),
            declaration(ThreadBuiltin::CreateDirectory)?,
            CreateDirectoryTool::new(workspace.clone()),
        )?
        .with_authorization(authority.clone()),
        Registration::new(
            "delete_path".into(),
            declaration(ThreadBuiltin::DeletePath)?,
            DeletePathTool::new(workspace.clone()),
        )?
        .with_authorization(authority.clone()),
        Registration::new(
            "copy_path".into(),
            declaration(ThreadBuiltin::CopyPath)?,
            CopyPathTool::new(workspace.clone()),
        )?
        .with_authorization(authority.clone()),
        Registration::new(
            "move_path".into(),
            declaration(ThreadBuiltin::MovePath)?,
            MovePathTool::new(workspace),
        )?
        .with_authorization(authority),
    ])
}

fn file_tools<B: pl_tool::workspace_file::WorkspaceFileBackend + 'static>(
    backend: Arc<B>,
    workspace: pl_tool::workspace::ToolWorkspace,
    media: Arc<super::media::MediaHost>,
) -> Result<Vec<Registration>, ThreadAssemblyError> {
    use pl_tool::workspace_file::{ThreadWorkspaceFileTool, WorkspaceFileToolKind};
    let mut tools = [
        WorkspaceFileToolKind::ReadFile,
        WorkspaceFileToolKind::ListFiles,
        WorkspaceFileToolKind::ApplyPatch,
    ]
    .into_iter()
    .map(|kind| {
        Ok(
            ThreadWorkspaceFileTool::new(kind, backend.clone(), workspace.clone())
                .registration(declaration(ThreadBuiltin::File(kind))?)?,
        )
    })
    .collect::<Result<Vec<_>, ThreadAssemblyError>>()?;
    tools.push(
        pl_tool::image::ThreadViewImageTool::new(backend.clone(), workspace.authorization(), media)
            .registration(declaration(ThreadBuiltin::ViewImage)?)?,
    );
    tools.push(
        pl_tool::workspace_file::ThreadStatPathTool::new(
            backend.clone(),
            workspace.authorization(),
        )
        .registration(declaration(ThreadBuiltin::StatPath)?)?,
    );
    tools.push(
        pl_tool::workspace_file::ThreadWriteFileTool::new(backend, workspace)
            .registration(declaration(ThreadBuiltin::WriteFile)?)?,
    );
    Ok(tools)
}

impl super::StudioThreadAssembler {
    /// Builds the standard catalog and transfers it through the same root/child/recovery owner path.
    ///
    /// # Errors
    /// Returns backend setup, declaration, history or resource-assembly failure.
    pub async fn assemble_standard(
        &self,
        mut spec: StudioThreadSpec,
        tools: StudioWorkspaceTools,
    ) -> Result<pl_core::thread::ThreadHandle, ThreadAssemblyError> {
        spec.agent_controls = if spec.parent_id.is_none() {
            super::AgentControlExposure::Enabled
        } else {
            super::AgentControlExposure::Disabled
        };
        let tools = StudioThreadTools::standard(tools).await?;
        self.assemble(tools.install(spec)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::thread::{ContextCapacity, ThreadLifecycle};
    use pretty_assertions::assert_eq;

    #[test]
    fn refreshed_command_catalog_shares_only_its_own_thread_manager() {
        let make = || {
            StudioCommandProcesses::Local(Arc::new(pl_tool::command::CommandProcessManager::new(
                Arc::new(pl_tool::command::LocalCommandBackend::new(
                    "/unused-command-binding",
                )),
            )))
        };
        let first = make();
        let other = make();
        let lease = first.downgrade();
        let refreshed = lease.upgrade().expect("live manager");
        if let (
            StudioCommandProcesses::Local(first),
            StudioCommandProcesses::Local(refreshed),
            StudioCommandProcesses::Local(other),
        ) = (&first, &refreshed, &other)
        {
            assert!(Arc::ptr_eq(first, refreshed));
            assert!(!Arc::ptr_eq(first, other));
        } else {
            panic!("expected local managers");
        }
        drop(first);
        assert!(lease.upgrade().is_some());
        drop(refreshed);
        assert!(
            lease.upgrade().is_none(),
            "weak cache cannot extend the physical lease"
        );
        assert!(
            other.downgrade().upgrade().is_some(),
            "another Thread is unaffected"
        );
    }

    #[tokio::test]
    async fn disabled_workspace_capabilities_do_not_activate_a_missing_physical_root() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("not-mounted");
        let tools = StudioThreadTools::standard(StudioWorkspaceTools {
            binding: StudioCommandBinding::Local {
                workspace: pl_tool::workspace::ToolWorkspace::new(
                    pl_tool::workspace::AgentWorkspace::local(missing.clone()),
                ),
                environment: pl_tool::environment::ExecutionEnvironment::detect_local(),
                access: CommandAccess::WorkspaceOnly,
            },
            store: FileResourceStore::new(directory.path().join("resources")),
            capabilities: pl_tool::workspace::ToolCapabilityConfig {
                exec: false,
                workspace_files: false,
                ask_user: false,
                skills: false,
                mcp: false,
                lsp: false,
                git: false,
            },
        })
        .await
        .expect("disabled backends must not resolve a workspace or worker");
        assert!(!missing.exists());
        let spec = super::super::tests::spec("disabled", None, directory.path());
        let assembler = super::super::StudioThreadAssembler::default();
        let thread = assembler.assemble(tools.install(spec)).await.unwrap();
        assert!(thread.snapshot().tasks.is_empty());
        assembler.close("disabled").await.unwrap();
    }

    #[tokio::test]
    async fn standard_assembly_binds_files_tasks_and_resources_once_without_starting_processes() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileResourceStore::new(directory.path().join("resources"));
        let assembler = super::super::StudioThreadAssembler::default();
        let route = pl_model::config::ResolvedModelRoute {
            pricing_mode: pl_protocol::PricingMode::Catalog,
            role: pl_protocol::AgentRoleId::new("test").unwrap(),
            provider_id: pl_model::config::ProviderId::new("test").unwrap(),
            endpoint: pl_model::provider::ProviderEndpoint::deepseek(None),
            model: pl_model::model::ModelInfo::compatible("assembly-test"),
            effort: None,
        };
        let thread = assembler
            .assemble_standard(
                StudioThreadSpec {
                    context_preparation: None,
                    agent_controls: super::super::AgentControlExposure::Disabled,
                    execution: pl_core::thread::input::InputDriverOptions {
                        max_model_steps: std::num::NonZeroU32::new(64).unwrap(),
                    },
                    hosted_tools: Vec::new(),
                    id: "standard".into(),
                    parent_id: None,
                    route,
                    history: Vec::new(),
                    initial_context: Vec::new(),
                    initial_extensions: Default::default(),
                    tools: Vec::new(),
                    resources: ResourceAccess::new(store.clone()),
                    capacity: ContextCapacity::default(),
                    cold_store: None,
                },
                StudioWorkspaceTools {
                    capabilities: pl_tool::workspace::ToolCapabilityConfig::default(),
                    binding: StudioCommandBinding::Local {
                        workspace: pl_tool::workspace::ToolWorkspace::new(
                            pl_tool::workspace::AgentWorkspace::local(directory.path().to_owned()),
                        ),
                        environment: pl_tool::environment::ExecutionEnvironment::detect_local(),
                        access: CommandAccess::WorkspaceOnly,
                    },
                    store,
                },
            )
            .await
            .unwrap();
        assert!(thread.snapshot().attempts.is_empty());
        assert!(thread.snapshot().tasks.is_empty());
        assert!(assembler.thread("standard").is_some());
        assert!(assembler.close_all().await.is_empty());
        assert_eq!(thread.snapshot().lifecycle, ThreadLifecycle::Closed);
    }
}

//! Installers for pl-core's built-in tools.

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::ToolCapabilityConfig;
use crate::tool::{
    AskUserTool, CommandBackend, CopyPathTool, CreateDirectoryTool, DeletePathTool,
    ExecutionBackend, GitCredentialProvider, GitTool, GitToolKind, GitWorkspaceConfig,
    LocalCommandBackend, LocalExecutionBackend, LocalWorkspaceFileBackend, LocalWorkspaceFileTool,
    MovePathTool, NoGitCredentialProvider, SessionNoteTool, SessionNoteToolKind, StatPathTool,
    TodoListTool, Tool, ToolGroupId, ToolWorkspace, WorkspaceFileBackend, WorkspaceFileTool,
    WorkspaceFileToolKind, WriteFileTool, command_tool_pair, local_command_tool_pair, lsp_tools,
};

use super::TurnEngine;

/// Installs the built-in tool implementations selected by explicit capabilities.
///
/// This type only constructs ordinary [`Tool`] implementations and publishes one
/// atomic group into the engine's persistent [`crate::AgentToolSet`].
#[derive(Debug, Clone)]
pub struct BuiltinToolInstaller<
    B = LocalExecutionBackend,
    P = NoGitCredentialProvider,
    E = LocalCommandBackend,
    W = LocalWorkspaceFileBackend,
> {
    capabilities: ToolCapabilityConfig,
    local_backends: bool,
    git_runtime: Option<GitToolRuntime<B, P>>,
    command_runtime: Option<CommandToolRuntime<E>>,
    workspace_file_runtime: Option<WorkspaceFileToolRuntime<W>>,
}

impl BuiltinToolInstaller {
    pub fn from_capabilities(capabilities: ToolCapabilityConfig) -> Self {
        Self {
            capabilities,
            local_backends: true,
            git_runtime: None,
            command_runtime: None,
            workspace_file_runtime: None,
        }
    }

    /// Constructs an installer that only uses explicitly injected host backends.
    pub fn host_provided(capabilities: ToolCapabilityConfig) -> Self {
        Self {
            capabilities,
            local_backends: false,
            git_runtime: None,
            command_runtime: None,
            workspace_file_runtime: None,
        }
    }
}

impl<B, P, E, W> BuiltinToolInstaller<B, P, E, W> {
    pub fn with_git_tools<NB, NP>(
        self,
        config: GitWorkspaceConfig,
        backend: Arc<NB>,
        credential_provider: Arc<NP>,
    ) -> BuiltinToolInstaller<NB, NP, E, W> {
        BuiltinToolInstaller {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: Some(GitToolRuntime {
                config,
                backend,
                credential_provider,
            }),
            command_runtime: self.command_runtime,
            workspace_file_runtime: self.workspace_file_runtime,
        }
    }

    pub fn with_command_backend<NE>(self, backend: Arc<NE>) -> BuiltinToolInstaller<B, P, NE, W> {
        BuiltinToolInstaller {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: Some(CommandToolRuntime { backend }),
            workspace_file_runtime: self.workspace_file_runtime,
        }
    }

    pub fn with_workspace_file_backend<NW>(
        self,
        backend: Arc<NW>,
    ) -> BuiltinToolInstaller<B, P, E, NW> {
        BuiltinToolInstaller {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: self.command_runtime,
            workspace_file_runtime: Some(WorkspaceFileToolRuntime { backend }),
        }
    }

    pub fn capabilities(&self) -> &ToolCapabilityConfig {
        &self.capabilities
    }
}

impl<B, P, E, W> BuiltinToolInstaller<B, P, E, W>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
    E: CommandBackend + 'static,
    W: WorkspaceFileBackend + 'static,
{
    pub async fn install(
        &self,
        core: &mut TurnEngine,
        workspace_root: impl Into<PathBuf>,
        workspace_instructions: Option<String>,
    ) -> crate::Result<()> {
        self.install_agent_workspace(
            core,
            crate::tool::AgentWorkspace::local(workspace_root),
            workspace_instructions,
        )
        .await
    }

    pub async fn install_agent_workspace(
        &self,
        core: &mut TurnEngine,
        workspace: crate::tool::AgentWorkspace,
        workspace_instructions: Option<String>,
    ) -> crate::Result<()> {
        let workspace_root = workspace.root().to_path_buf();
        let tool_workspace =
            ToolWorkspace::new(workspace.clone()).with_lsp_runtime(core.lsp_runtime.clone());
        core.workspace = Some(workspace);
        core.workspace_instructions = workspace_instructions;
        let mut tools = Vec::<Arc<dyn Tool>>::new();

        if self.capabilities.exec && self.local_backends {
            let (exec, write_stdin) = local_command_tool_pair(tool_workspace.clone());
            tools.push(Arc::new(exec));
            tools.push(Arc::new(write_stdin));
        }
        if self.capabilities.exec
            && let Some(runtime) = &self.command_runtime
        {
            let (exec, write_stdin) =
                command_tool_pair(runtime.backend.clone(), tool_workspace.clone());
            tools.push(Arc::new(exec));
            tools.push(Arc::new(write_stdin));
        }
        if self.capabilities.workspace_files && self.local_backends {
            tools.extend(WorkspaceFileToolKind::all().iter().map(|kind| {
                Arc::new(LocalWorkspaceFileTool::new(*kind, tool_workspace.clone()))
                    as Arc<dyn Tool>
            }));
            tools.extend([
                Arc::new(WriteFileTool::new(tool_workspace.clone())) as Arc<dyn Tool>,
                Arc::new(StatPathTool::new(tool_workspace.clone())),
                Arc::new(CreateDirectoryTool::new(tool_workspace.clone())),
                Arc::new(DeletePathTool::new(tool_workspace.clone())),
                Arc::new(CopyPathTool::new(tool_workspace.clone())),
                Arc::new(MovePathTool::new(tool_workspace.clone())),
            ]);
        }
        if self.capabilities.workspace_files
            && let Some(runtime) = &self.workspace_file_runtime
        {
            tools.extend(WorkspaceFileToolKind::all().iter().map(|kind| {
                Arc::new(WorkspaceFileTool::new(*kind, runtime.backend.clone())) as Arc<dyn Tool>
            }));
        }
        if self.capabilities.ask_user {
            tools.push(Arc::new(AskUserTool));
        }
        tools.push(Arc::new(TodoListTool::new(
            core.tool_session_runtime.working_set(),
        )));
        tools.extend(SessionNoteToolKind::all().iter().map(|kind| {
            Arc::new(SessionNoteTool::new(
                *kind,
                core.tool_session_runtime.working_set(),
            )) as Arc<dyn Tool>
        }));
        if self.capabilities.git
            && let Some(runtime) = &self.git_runtime
        {
            tools.extend(git_tools(runtime));
        }
        core.agent_tools
            .install(ToolGroupId::new("builtin"), tools)?;

        if self.capabilities.skills {
            core.register_skill_tools_for_workspace(workspace_root.clone())?;
        } else {
            core.agent_tools.uninstall(&ToolGroupId::new("skills"));
        }

        if self.capabilities.lsp
            && let Some(registry) = core.lsp_runtime.clone()
            && !registry
                .active_server_names_for_workspace(&workspace_root)
                .await
                .is_empty()
        {
            core.agent_tools
                .install(ToolGroupId::new("lsp"), lsp_tools(registry, tool_workspace))?;
        } else {
            core.agent_tools.uninstall(&ToolGroupId::new("lsp"));
        }
        Ok(())
    }
}

fn git_tools<B, P>(runtime: &GitToolRuntime<B, P>) -> Vec<Arc<dyn Tool>>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
{
    GitToolKind::all()
        .iter()
        .copied()
        .map(|kind| {
            Arc::new(GitTool::new(
                kind,
                runtime.config.clone(),
                runtime.backend.clone(),
                runtime.credential_provider.clone(),
            )) as Arc<dyn Tool>
        })
        .collect()
}

#[derive(Debug, Clone)]
struct GitToolRuntime<B, P> {
    config: GitWorkspaceConfig,
    backend: Arc<B>,
    credential_provider: Arc<P>,
}

#[derive(Debug, Clone)]
struct CommandToolRuntime<E> {
    backend: Arc<E>,
}

#[derive(Debug, Clone)]
struct WorkspaceFileToolRuntime<W> {
    backend: Arc<W>,
}

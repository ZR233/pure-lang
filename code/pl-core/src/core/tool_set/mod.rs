//! Installers for pl-core's built-in tools.

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::ToolCapabilityConfig;
use crate::tool::{
    AskUserTool, CommandBackend, CopyPathTool, CreateDirectoryTool, DeletePathTool, DynTool,
    ExecutionBackend, GitCredentialProvider, GitTool, GitToolKind, GitWorkspaceConfig,
    LocalCommandBackend, LocalExecutionBackend, LocalWorkspaceFileBackend, LocalWorkspaceFileTool,
    MovePathTool, NoGitCredentialProvider, SessionNoteTool, SessionNoteToolKind, StatPathTool,
    SubmitPlanTool, TodoListTool, ToolGroupId, ToolInstallGroup, ToolWorkspace,
    WorkspaceFileBackend, WorkspaceFileTool, WorkspaceFileToolKind, WorkspacePolicyBackend,
    WriteFileTool, command_tool_pair, local_command_tool_pair_with_environment, lsp_tools,
};

use super::TurnEngine;

/// Installs the built-in tool implementations selected by explicit capabilities.
///
/// This type only constructs ordinary [`crate::StaticTool`] implementations and publishes one
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
    additional_tools: Vec<DynTool>,
}

impl BuiltinToolInstaller {
    pub fn from_capabilities(capabilities: ToolCapabilityConfig) -> Self {
        Self {
            capabilities,
            local_backends: true,
            git_runtime: None,
            command_runtime: None,
            workspace_file_runtime: None,
            additional_tools: Vec::new(),
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
            additional_tools: Vec::new(),
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
            additional_tools: self.additional_tools,
        }
    }

    pub fn with_command_backend<NE>(self, backend: Arc<NE>) -> BuiltinToolInstaller<B, P, NE, W> {
        BuiltinToolInstaller {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: Some(CommandToolRuntime { backend }),
            workspace_file_runtime: self.workspace_file_runtime,
            additional_tools: self.additional_tools,
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
            additional_tools: self.additional_tools,
        }
    }

    pub fn with_additional_tools(mut self, tools: Vec<DynTool>) -> Self {
        self.additional_tools.extend(tools);
        self
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
        let mut tools = Vec::<DynTool>::new();

        if self.capabilities.exec && self.local_backends {
            let (exec, write_stdin) = local_command_tool_pair_with_environment(
                tool_workspace.clone(),
                core.execution_environment.clone(),
            );
            tools.push(exec.into());
            tools.push(write_stdin.into());
        }
        if self.capabilities.exec
            && let Some(runtime) = &self.command_runtime
        {
            let (exec, write_stdin) =
                command_tool_pair(runtime.backend.clone(), tool_workspace.clone());
            tools.push(exec.into());
            tools.push(write_stdin.into());
        }
        if self.capabilities.workspace_files && self.local_backends {
            tools.extend(
                WorkspaceFileToolKind::all()
                    .iter()
                    .map(|kind| LocalWorkspaceFileTool::new(*kind, tool_workspace.clone()).into()),
            );
            tools.extend([
                WriteFileTool::new(tool_workspace.clone()).into(),
                StatPathTool::new(tool_workspace.clone()).into(),
                CreateDirectoryTool::new(tool_workspace.clone()).into(),
                DeletePathTool::new(tool_workspace.clone()).into(),
                CopyPathTool::new(tool_workspace.clone()).into(),
                MovePathTool::new(tool_workspace.clone()).into(),
            ]);
        }
        if self.capabilities.workspace_files
            && let Some(runtime) = &self.workspace_file_runtime
        {
            let backend = Arc::new(WorkspacePolicyBackend::new(
                runtime.backend.clone(),
                tool_workspace.clone(),
            ));
            tools.extend(
                WorkspaceFileToolKind::all()
                    .iter()
                    .map(|kind| WorkspaceFileTool::new(*kind, backend.clone()).into()),
            );
        }
        tools.extend(self.additional_tools.iter().cloned());
        if self.capabilities.ask_user {
            tools.push(AskUserTool.into());
            tools.push(SubmitPlanTool.into());
        }
        tools.push(TodoListTool::new(core.tool_session_runtime.working_set()).into());
        tools.extend(SessionNoteToolKind::all().iter().map(|kind| {
            SessionNoteTool::new(*kind, core.tool_session_runtime.working_set()).into()
        }));
        if self.capabilities.git
            && let Some(runtime) = &self.git_runtime
        {
            tools.extend(git_tools(runtime));
        }
        core.agent_tools
            .install(ToolInstallGroup::direct(ToolGroupId::new("builtin"), tools))?;

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
            core.agent_tools.install(ToolInstallGroup::direct(
                ToolGroupId::new("lsp"),
                lsp_tools(registry, tool_workspace),
            ))?;
        } else {
            core.agent_tools.uninstall(&ToolGroupId::new("lsp"));
        }
        Ok(())
    }
}

fn git_tools<B, P>(runtime: &GitToolRuntime<B, P>) -> Vec<DynTool>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
{
    GitToolKind::all()
        .iter()
        .copied()
        .map(|kind| {
            GitTool::new(
                kind,
                runtime.config.clone(),
                runtime.backend.clone(),
                runtime.credential_provider.clone(),
            )
            .into()
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

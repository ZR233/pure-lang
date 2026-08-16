use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use pl_model::ToolSchema;

use crate::config::ToolCapabilityConfig;
use crate::tool::{
    AskUserTool, CommandBackend, CopyPathTool, CreateDirectoryTool, DeletePathTool,
    ExecutionBackend, GitCredentialProvider, GitToolKind, GitWorkspaceConfig, LocalCommandBackend,
    LocalExecutionBackend, LocalWorkspaceFileBackend, LocalWorkspaceFileTool, MovePathTool,
    NoGitCredentialProvider, PlanExitTool, SessionNoteTool, SessionNoteToolKind, StatPathTool,
    TodoListTool, Tool, WorkspaceFileBackend, WorkspaceFileTool, WorkspaceFileToolKind,
    WriteFileTool, command_tool_pair, local_command_tool_pair, lsp_tool_entries,
};
use crate::tool::{ToolEntry, ToolSourceId, ToolSourceMetadata};

use super::TurnEngine;

mod visibility;
pub use visibility::{SharedToolSchemaOptions, ToolVisibilitySet};

/// 按能力开关组装通用工具；agent 协作工具由 `AgentRuntimeHandle` 按 turn 注入。
#[derive(Debug, Clone)]
pub struct ToolSetBuilder<
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
    allowed_tools: Option<HashSet<String>>,
}

impl ToolSetBuilder {
    pub fn from_capabilities(capabilities: ToolCapabilityConfig) -> Self {
        Self {
            capabilities,
            local_backends: true,
            git_runtime: None,
            command_runtime: None,
            workspace_file_runtime: None,
            allowed_tools: None,
        }
    }

    /// 构造只使用宿主显式注入 backend 的工具集合。
    pub fn host_provided(capabilities: ToolCapabilityConfig) -> Self {
        Self {
            capabilities,
            local_backends: false,
            git_runtime: None,
            command_runtime: None,
            workspace_file_runtime: None,
            allowed_tools: None,
        }
    }
}

impl<B, P, E, W> ToolSetBuilder<B, P, E, W> {
    pub fn with_allowed_tools<I, S>(mut self, allowed_tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_tools = Some(allowed_tools.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_git_tools<NB, NP>(
        self,
        config: GitWorkspaceConfig,
        backend: Arc<NB>,
        credential_provider: Arc<NP>,
    ) -> ToolSetBuilder<NB, NP, E, W> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: Some(GitToolRuntime {
                config,
                backend,
                credential_provider,
            }),
            command_runtime: self.command_runtime,
            workspace_file_runtime: self.workspace_file_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_command_backend<NE>(self, backend: Arc<NE>) -> ToolSetBuilder<B, P, NE, W> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: Some(CommandToolRuntime { backend }),
            workspace_file_runtime: self.workspace_file_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_workspace_file_backend<NW>(self, backend: Arc<NW>) -> ToolSetBuilder<B, P, E, NW> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: self.command_runtime,
            workspace_file_runtime: Some(WorkspaceFileToolRuntime { backend }),
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn capabilities(&self) -> &ToolCapabilityConfig {
        &self.capabilities
    }

    pub fn shared_tool_schemas(&self) -> Vec<ToolSchema> {
        let mut options = SharedToolSchemaOptions::from_capabilities(&self.capabilities);
        if !self.local_backends && self.workspace_file_runtime.is_none() {
            options.workspace_files = false;
        }
        if !self.local_backends && self.command_runtime.is_none() {
            options.exec = false;
        }
        options.git = options.git && self.git_runtime.is_some();
        let mut schemas = shared_tool_schemas(options);
        if let Some(allowed) = &self.allowed_tools {
            schemas.retain(|schema| allowed.contains(schema.name()));
        }
        schemas
    }

    pub fn shared_tool_names(&self) -> Vec<String> {
        self.shared_tool_schemas()
            .into_iter()
            .map(|schema| schema.name().to_string())
            .collect()
    }

    fn tool_allowed(&self, name: &str) -> bool {
        self.allowed_tools
            .as_ref()
            .is_none_or(|allowed| allowed.contains(name))
    }
}

impl<B, P, E, W> ToolSetBuilder<B, P, E, W>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
    E: CommandBackend,
    W: WorkspaceFileBackend + 'static,
{
    pub async fn register(
        &self,
        core: &mut TurnEngine,
        workspace_root: impl Into<PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        self.register_agent_workspace(
            core,
            crate::tool::AgentWorkspace::local(workspace_root),
            workspace_instructions,
        )
        .await;
    }

    pub async fn register_agent_workspace(
        &self,
        core: &mut TurnEngine,
        workspace: crate::tool::AgentWorkspace,
        workspace_instructions: Option<String>,
    ) {
        let workspace_root = workspace.root().to_path_buf();
        core.workspace = Some(workspace);
        core.workspace_instructions = workspace_instructions.clone();
        let source = ToolSourceId::builtin();
        let mut entries = Vec::new();
        if self.capabilities.exec && self.local_backends {
            let (exec, write_stdin) = local_command_tool_pair(workspace_root.clone());
            push_if_allowed(&mut entries, exec, &source, |name| self.tool_allowed(name));
            push_if_allowed(&mut entries, write_stdin, &source, |name| {
                self.tool_allowed(name)
            });
        }
        if self.capabilities.exec
            && let Some(runtime) = &self.command_runtime
        {
            let (exec, write_stdin) = command_tool_pair(runtime.backend.clone());
            push_if_allowed(&mut entries, exec, &source, |name| self.tool_allowed(name));
            push_if_allowed(&mut entries, write_stdin, &source, |name| {
                self.tool_allowed(name)
            });
        }
        if self.capabilities.workspace_files && self.local_backends {
            for kind in WorkspaceFileToolKind::all() {
                push_if_allowed(
                    &mut entries,
                    LocalWorkspaceFileTool::new(*kind),
                    &source,
                    |name| self.tool_allowed(name),
                );
            }
            let mut file_tools: Vec<Box<dyn Tool>> = vec![
                Box::new(WriteFileTool),
                Box::new(StatPathTool),
                Box::new(CreateDirectoryTool),
                Box::new(DeletePathTool),
                Box::new(CopyPathTool),
                Box::new(MovePathTool),
            ];
            for tool in file_tools.drain(..) {
                push_if_allowed_boxed(&mut entries, tool, &source, |name| self.tool_allowed(name));
            }
        }
        if self.capabilities.workspace_files
            && let Some(runtime) = &self.workspace_file_runtime
        {
            for kind in WorkspaceFileToolKind::all() {
                let name = kind.name();
                if self.tool_allowed(name) {
                    entries.push(ToolEntry::new(
                        WorkspaceFileTool::new(*kind, runtime.backend.clone()),
                        ToolSourceMetadata::new(source.clone()),
                    ));
                }
            }
        }
        if self.capabilities.ask_user {
            push_if_allowed(&mut entries, AskUserTool, &source, |name| {
                self.tool_allowed(name)
            });
        }
        push_if_allowed(&mut entries, TodoListTool, &source, |name| {
            self.tool_allowed(name)
        });
        for kind in SessionNoteToolKind::all() {
            push_if_allowed(&mut entries, SessionNoteTool::new(*kind), &source, |name| {
                self.tool_allowed(name)
            });
        }
        push_if_allowed(&mut entries, PlanExitTool, &source, |name| {
            self.tool_allowed(name)
        });
        if self.capabilities.git
            && let Some(runtime) = &self.git_runtime
        {
            entries.extend(git_entries(runtime, |name| self.tool_allowed(name)));
        }
        let _ = core.register_source_tools(source, entries);
        if self.capabilities.skills {
            core.register_skill_tools_for_workspace(workspace_root.clone());
        }
        if self.capabilities.lsp
            && let Some(registry) = core.lsp_runtime.clone()
        {
            let _ = core.register_source_tools(ToolSourceId::lsp(), lsp_tool_entries(registry));
        }
    }
}

fn push_if_allowed<T>(
    entries: &mut Vec<ToolEntry>,
    tool: T,
    source: &ToolSourceId,
    allowed: impl Fn(&str) -> bool,
) where
    T: Tool + 'static,
{
    if allowed(tool.name()) {
        entries.push(ToolEntry::new(
            tool,
            ToolSourceMetadata::new(source.clone()),
        ));
    }
}

fn push_if_allowed_boxed(
    entries: &mut Vec<ToolEntry>,
    tool: Box<dyn Tool>,
    source: &ToolSourceId,
    allowed: impl Fn(&str) -> bool,
) {
    if allowed(tool.name()) {
        entries.push(ToolEntry::from_arc(
            Arc::from(tool),
            ToolSourceMetadata::new(source.clone()),
        ));
    }
}

fn git_entries<B, P>(
    runtime: &GitToolRuntime<B, P>,
    allowed: impl Fn(&str) -> bool + Copy,
) -> Vec<ToolEntry>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
{
    let namespace = Some(crate::tool::NamespaceDescriptor::new(
        "git",
        "Git inspection and repository management tools.",
    ));
    GitToolKind::all()
        .iter()
        .copied()
        .filter(|kind| allowed(kind.name()))
        .map(|kind| {
            let programmatic = matches!(kind.effect(), crate::turn::ToolEffect::Read);
            let metadata = ToolSourceMetadata {
                source: ToolSourceId::builtin(),
                namespace: namespace.clone(),
                programmatic_eligible: programmatic,
            };
            ToolEntry::new(
                crate::tool::GitTool::new(
                    kind,
                    runtime.config.clone(),
                    runtime.backend.clone(),
                    runtime.credential_provider.clone(),
                ),
                metadata,
            )
        })
        .collect()
}

pub fn shared_tool_schemas(options: SharedToolSchemaOptions) -> Vec<ToolSchema> {
    let mut schemas = Vec::new();
    if options.exec {
        let (exec, write_stdin) = local_command_tool_pair(PathBuf::new());
        schemas.push(exec.to_schema());
        schemas.push(write_stdin.to_schema());
    }
    if options.workspace_files {
        schemas.extend(
            WorkspaceFileToolKind::all()
                .iter()
                .copied()
                .map(WorkspaceFileToolKind::to_schema),
        );
    }
    if options.ask_user {
        schemas.push(AskUserTool.to_schema());
    }
    if options.todo {
        schemas.push(TodoListTool.to_schema());
    }
    schemas.extend(
        SessionNoteToolKind::all()
            .iter()
            .copied()
            .map(|kind| SessionNoteTool::new(kind).to_schema()),
    );
    if options.git {
        schemas.extend(
            GitToolKind::all()
                .iter()
                .copied()
                .map(GitToolKind::to_schema),
        );
    }
    if options.plan_exit {
        schemas.push(PlanExitTool.to_schema());
    }
    schemas
}

pub fn shared_tool_names(options: SharedToolSchemaOptions) -> Vec<String> {
    shared_tool_schemas(options)
        .into_iter()
        .map(|schema| schema.name().to_string())
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

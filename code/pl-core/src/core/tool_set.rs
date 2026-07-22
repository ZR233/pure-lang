use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use pl_model::ToolSchema;
use serde_json::Value;

use crate::config::ToolCapabilityConfig;
use crate::tool::{
    AskUserTool, CommandBackend, CopyPathTool, CreateDirectoryTool, DeletePathTool,
    ExecutionBackend, GitCredentialProvider, GitToolKind, GitWorkspaceConfig, LocalCommandBackend,
    LocalExecutionBackend, LocalWorkspaceFileBackend, LocalWorkspaceFileTool,
    McpListResourceTemplatesRequest, McpListResourcesRequest, McpReadResourceRequest,
    McpResourceBackend, McpResourceTool, McpResourceToolKind, McpTool, McpToolBackend,
    McpToolRequest, MovePathTool, NoGitCredentialProvider, PlanExitTool, SessionNoteTool,
    SessionNoteToolKind, StatPathTool, TodoListTool, Tool, WorkspaceFileBackend, WorkspaceFileTool,
    WorkspaceFileToolKind, WriteFileTool, command_tool_pair, local_command_tool_pair,
};

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
    M = NoMcpResourceBackend,
    T = NoMcpToolBackend,
> {
    capabilities: ToolCapabilityConfig,
    local_backends: bool,
    git_runtime: Option<GitToolRuntime<B, P>>,
    command_runtime: Option<CommandToolRuntime<E>>,
    workspace_file_runtime: Option<WorkspaceFileToolRuntime<W>>,
    mcp_resource_runtime: Option<McpResourceToolRuntime<M>>,
    mcp_tool_runtime: Option<McpToolRuntime<T>>,
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
            mcp_resource_runtime: None,
            mcp_tool_runtime: None,
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
            mcp_resource_runtime: None,
            mcp_tool_runtime: None,
            allowed_tools: None,
        }
    }
}

impl<B, P, E, W, M, T> ToolSetBuilder<B, P, E, W, M, T> {
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
    ) -> ToolSetBuilder<NB, NP, E, W, M, T> {
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
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_command_backend<NE>(self, backend: Arc<NE>) -> ToolSetBuilder<B, P, NE, W, M, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: Some(CommandToolRuntime { backend }),
            workspace_file_runtime: self.workspace_file_runtime,
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_workspace_file_backend<NW>(
        self,
        backend: Arc<NW>,
    ) -> ToolSetBuilder<B, P, E, NW, M, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: self.command_runtime,
            workspace_file_runtime: Some(WorkspaceFileToolRuntime { backend }),
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_mcp_resource_tools<NM>(
        self,
        backend: Arc<NM>,
    ) -> ToolSetBuilder<B, P, E, W, NM, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: self.command_runtime,
            workspace_file_runtime: self.workspace_file_runtime,
            mcp_resource_runtime: Some(McpResourceToolRuntime { backend }),
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_mcp_tools<NT>(
        self,
        schemas: Vec<ToolSchema>,
        backend: Arc<NT>,
    ) -> ToolSetBuilder<B, P, E, W, M, NT> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            local_backends: self.local_backends,
            git_runtime: self.git_runtime,
            command_runtime: self.command_runtime,
            workspace_file_runtime: self.workspace_file_runtime,
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: Some(McpToolRuntime { schemas, backend }),
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
        options.mcp_resources = options.mcp_resources && self.mcp_resource_runtime.is_some();
        let mut schemas = shared_tool_schemas(options);
        if self.capabilities.mcp
            && let Some(runtime) = &self.mcp_tool_runtime
        {
            schemas.extend(runtime.schemas.clone());
        }
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

impl<B, P, E, W, M, T> ToolSetBuilder<B, P, E, W, M, T>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
    E: CommandBackend,
    W: WorkspaceFileBackend + 'static,
    M: McpResourceBackend + 'static,
    T: McpToolBackend + 'static,
{
    pub async fn register(
        &self,
        core: &mut TurnEngine,
        workspace_root: impl Into<PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        let workspace_root = workspace_root.into();
        core.workspace_root = Some(workspace_root.clone());
        core.workspace_instructions = workspace_instructions.clone();
        if self.capabilities.skills {
            core.register_skill_tools_for_workspace(
                workspace_root.clone(),
                workspace_instructions.clone(),
            );
        }
        if self.capabilities.exec && self.local_backends {
            let (exec, write_stdin) = local_command_tool_pair(workspace_root.clone());
            register_if_allowed(core, exec, |name| self.tool_allowed(name));
            register_if_allowed(core, write_stdin, |name| self.tool_allowed(name));
        }
        if self.capabilities.exec
            && let Some(runtime) = &self.command_runtime
        {
            let (exec, write_stdin) = command_tool_pair(runtime.backend.clone());
            register_if_allowed(core, exec, |name| self.tool_allowed(name));
            register_if_allowed(core, write_stdin, |name| self.tool_allowed(name));
        }
        if self.capabilities.workspace_files && self.local_backends {
            register_file_tools(core, |name| self.tool_allowed(name));
        }
        if self.capabilities.workspace_files
            && let Some(runtime) = &self.workspace_file_runtime
        {
            register_workspace_file_tools(core, runtime.backend.clone(), |name| {
                self.tool_allowed(name)
            });
        }
        if self.capabilities.lsp
            && let Some(registry) = core.lsp_runtime.clone()
        {
            core.tools.register_lsp_languages(&registry).await;
        }
        if self.capabilities.ask_user {
            register_if_allowed(core, AskUserTool, |name| self.tool_allowed(name));
        }
        if self.capabilities.git
            && let Some(runtime) = &self.git_runtime
        {
            for kind in GitToolKind::all() {
                if self.tool_allowed(kind.name()) {
                    core.register_tool(crate::tool::GitTool::new(
                        *kind,
                        runtime.config.clone(),
                        runtime.backend.clone(),
                        runtime.credential_provider.clone(),
                    ));
                }
            }
        }
        if self.capabilities.mcp
            && let Some(runtime) = &self.mcp_resource_runtime
        {
            register_mcp_resource_tools(core, runtime.backend.clone(), |name| {
                self.tool_allowed(name)
            });
        }
        if self.capabilities.mcp
            && let Some(runtime) = &self.mcp_tool_runtime
        {
            register_mcp_tools(
                core,
                runtime.schemas.clone(),
                runtime.backend.clone(),
                |name| self.tool_allowed(name),
            );
        }
        register_if_allowed(core, TodoListTool, |name| self.tool_allowed(name));
        for kind in SessionNoteToolKind::all() {
            register_if_allowed(core, SessionNoteTool::new(*kind), |name| {
                self.tool_allowed(name)
            });
        }
        register_if_allowed(core, PlanExitTool, |name| self.tool_allowed(name));
    }
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
    if options.mcp_resources {
        schemas.extend(
            McpResourceToolKind::all()
                .iter()
                .copied()
                .map(McpResourceToolKind::to_schema),
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

#[derive(Debug, Clone)]
struct McpResourceToolRuntime<M> {
    backend: Arc<M>,
}

#[derive(Debug, Clone)]
struct McpToolRuntime<T> {
    schemas: Vec<ToolSchema>,
    backend: Arc<T>,
}

#[derive(Debug, Clone, Default)]
pub struct NoMcpResourceBackend;

impl McpResourceBackend for NoMcpResourceBackend {
    type Error = String;

    async fn list_resources(
        &self,
        _request: McpListResourcesRequest,
    ) -> Result<Value, Self::Error> {
        Err("MCP resource backend is not configured".to_string())
    }

    async fn list_resource_templates(
        &self,
        _request: McpListResourceTemplatesRequest,
    ) -> Result<Value, Self::Error> {
        Err("MCP resource backend is not configured".to_string())
    }

    async fn read_resource(&self, _request: McpReadResourceRequest) -> Result<Value, Self::Error> {
        Err("MCP resource backend is not configured".to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoMcpToolBackend;

impl McpToolBackend for NoMcpToolBackend {
    type Error = String;

    async fn call_tool(&self, _request: McpToolRequest) -> Result<Value, Self::Error> {
        Err("MCP tool backend is not configured".to_string())
    }
}

fn register_if_allowed(
    core: &mut TurnEngine,
    tool: impl crate::tool::Tool + 'static,
    allowed: impl Fn(&str) -> bool,
) {
    if allowed(tool.name()) {
        core.register_tool(tool);
    }
}

fn register_file_tools(core: &mut TurnEngine, allowed: impl Fn(&str) -> bool + Copy) {
    for kind in WorkspaceFileToolKind::all() {
        register_if_allowed(core, LocalWorkspaceFileTool::new(*kind), allowed);
    }
    register_if_allowed(core, WriteFileTool, allowed);
    register_if_allowed(core, StatPathTool, allowed);
    register_if_allowed(core, CreateDirectoryTool, allowed);
    register_if_allowed(core, DeletePathTool, allowed);
    register_if_allowed(core, CopyPathTool, allowed);
    register_if_allowed(core, MovePathTool, allowed);
}

fn register_workspace_file_tools<W>(
    core: &mut TurnEngine,
    backend: Arc<W>,
    allowed: impl Fn(&str) -> bool,
) where
    W: WorkspaceFileBackend + 'static,
{
    for kind in WorkspaceFileToolKind::all() {
        if allowed(kind.name()) {
            core.register_tool(WorkspaceFileTool::new(*kind, backend.clone()));
        }
    }
}

fn register_mcp_resource_tools<M>(
    core: &mut TurnEngine,
    backend: Arc<M>,
    allowed: impl Fn(&str) -> bool,
) where
    M: McpResourceBackend + 'static,
{
    for kind in McpResourceToolKind::all() {
        if allowed(kind.name()) {
            core.register_tool(McpResourceTool::new(*kind, backend.clone()));
        }
    }
}

fn register_mcp_tools<T>(
    core: &mut TurnEngine,
    schemas: Vec<ToolSchema>,
    backend: Arc<T>,
    allowed: impl Fn(&str) -> bool,
) where
    T: McpToolBackend + 'static,
{
    for schema in schemas {
        if allowed(schema.name()) {
            core.register_tool(McpTool::new(schema, backend.clone()));
        }
    }
}

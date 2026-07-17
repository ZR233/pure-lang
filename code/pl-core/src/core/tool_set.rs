use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use pl_model::ToolSchema;
use serde_json::Value;

use crate::config::ToolCapabilityConfig;
use crate::tool::{
    AskUserTool, ContainerBackend, ContainerToolKind, ContainerWorkspaceFileBackend, CopyPathTool,
    CreateDirectoryTool, DeletePathTool, ExecutionBackend, GitCredentialProvider, GitToolKind,
    GitWorkspaceConfig, LocalExecutionBackend, LocalWorkspaceFileTool,
    McpListResourceTemplatesRequest, McpListResourcesRequest, McpReadResourceRequest,
    McpResourceBackend, McpResourceTool, McpResourceToolKind, McpTool, McpToolBackend,
    McpToolRequest, MovePathTool, NoContainerBackend, NoGitCredentialProvider, PlanExitTool,
    StatPathTool, TodoListTool, Tool, WorkspaceFileTool, WorkspaceFileToolKind, WriteFileTool,
    command_tool_pair,
};

use super::TurnEngine;

mod visibility;
pub use visibility::{SharedToolSchemaOptions, ToolVisibilitySet};

/// 按能力开关组装通用工具；agent 协作工具由 `AgentRuntimeHandle` 按 turn 注入。
#[derive(Debug, Clone, Default)]
pub struct ToolSetBuilder<
    B = LocalExecutionBackend,
    P = NoGitCredentialProvider,
    C = NoContainerBackend,
    M = NoMcpResourceBackend,
    T = NoMcpToolBackend,
> {
    capabilities: ToolCapabilityConfig,
    git_runtime: Option<GitToolRuntime<B, P>>,
    container_runtime: Option<ContainerToolRuntime<C>>,
    mcp_resource_runtime: Option<McpResourceToolRuntime<M>>,
    mcp_tool_runtime: Option<McpToolRuntime<T>>,
    allowed_tools: Option<HashSet<String>>,
}

impl ToolSetBuilder {
    pub fn from_capabilities(capabilities: ToolCapabilityConfig) -> Self {
        Self {
            capabilities,
            git_runtime: None,
            container_runtime: None,
            mcp_resource_runtime: None,
            mcp_tool_runtime: None,
            allowed_tools: None,
        }
    }
}

impl<B, P, C, M, T> ToolSetBuilder<B, P, C, M, T> {
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
    ) -> ToolSetBuilder<NB, NP, C, M, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: Some(GitToolRuntime {
                config,
                backend,
                credential_provider,
            }),
            container_runtime: self.container_runtime,
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_container_tools<NC>(self, backend: Arc<NC>) -> ToolSetBuilder<B, P, NC, M, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: self.git_runtime,
            container_runtime: Some(ContainerToolRuntime { backend }),
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_mcp_resource_tools<NM>(self, backend: Arc<NM>) -> ToolSetBuilder<B, P, C, NM, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: self.git_runtime,
            container_runtime: self.container_runtime,
            mcp_resource_runtime: Some(McpResourceToolRuntime { backend }),
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_mcp_tools<NT>(
        self,
        schemas: Vec<ToolSchema>,
        backend: Arc<NT>,
    ) -> ToolSetBuilder<B, P, C, M, NT> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: self.git_runtime,
            container_runtime: self.container_runtime,
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
        if self.capabilities.container && self.container_runtime.is_none() {
            options.workspace_files = false;
        }
        options.git = options.git && self.git_runtime.is_some();
        options.container = options.container && self.container_runtime.is_some();
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

impl<B, P, C, M, T> ToolSetBuilder<B, P, C, M, T>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
    C: ContainerBackend + 'static,
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
        if self.capabilities.bash {
            let (bash, write_stdin) = command_tool_pair(workspace_root.clone());
            register_if_allowed(core, bash, |name| self.tool_allowed(name));
            register_if_allowed(core, write_stdin, |name| self.tool_allowed(name));
        }
        let container_workspace = self.capabilities.container && self.container_runtime.is_some();
        if self.capabilities.workspace_files && !self.capabilities.container {
            register_file_tools(core, |name| self.tool_allowed(name));
        }
        if self.capabilities.workspace_files
            && container_workspace
            && let Some(runtime) = &self.container_runtime
        {
            register_container_file_tools(core, runtime.backend.clone(), |name| {
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
        if self.capabilities.container
            && let Some(runtime) = &self.container_runtime
        {
            for kind in ContainerToolKind::all() {
                if self.tool_allowed(kind.name()) {
                    core.register_tool(crate::tool::ContainerTool::new(
                        *kind,
                        runtime.backend.clone(),
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
        register_if_allowed(core, PlanExitTool, |name| self.tool_allowed(name));
    }
}

pub fn shared_tool_schemas(options: SharedToolSchemaOptions) -> Vec<ToolSchema> {
    let mut schemas = Vec::new();
    if options.bash {
        let (bash, write_stdin) = command_tool_pair(PathBuf::new());
        schemas.push(bash.to_schema());
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
    if options.git {
        schemas.extend(
            GitToolKind::all()
                .iter()
                .copied()
                .map(GitToolKind::to_schema),
        );
    }
    if options.container {
        schemas.extend(
            ContainerToolKind::all()
                .iter()
                .copied()
                .map(ContainerToolKind::to_schema),
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
struct ContainerToolRuntime<C> {
    backend: Arc<C>,
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

fn register_container_file_tools<C>(
    core: &mut TurnEngine,
    backend: Arc<C>,
    allowed: impl Fn(&str) -> bool,
) where
    C: ContainerBackend + 'static,
{
    let backend = Arc::new(ContainerWorkspaceFileBackend::new(backend));
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

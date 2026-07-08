use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::ToolCapabilityConfig;
use crate::tool::{
    AgentControlBackend, AgentControlListOutput, AgentControlListRequest,
    AgentControlMessageOutput, AgentControlSendInputOutput, AgentControlSendInputRequest,
    AgentControlSpawnOutput, AgentControlSpawnRequest, AgentControlTargetRequest, AgentControlTool,
    AgentControlToolKind, AgentControlWaitOutput, AgentControlWaitRequest, ApplyPatchTool,
    AskUserTool, CloseAgentTool, ContainerBackend, ContainerToolKind,
    ContainerWorkspaceFileBackend, CopyPathTool, CreateDirectoryTool, DeletePathTool,
    ExecutionBackend, GitCredentialProvider, GitToolKind, GitWorkspaceConfig, ListAgentsTool,
    ListFilesTool, LocalExecutionBackend, McpListResourceTemplatesRequest, McpListResourcesRequest,
    McpReadResourceRequest, McpResourceBackend, McpResourceTool, McpResourceToolKind, McpTool,
    McpToolBackend, McpToolRequest, MovePathTool, NoContainerBackend, NoGitCredentialProvider,
    PlanExitTool, ReadFileTool, ResumeAgentTool, SearchFilesTool, SendInputTool, SpawnAgentTool,
    StatPathTool, TodoListTool, Tool, WaitAgentTool, WorkspaceFileTool, WorkspaceFileToolKind,
    WriteFileTool, command_tool_pair,
};
use pl_model::ToolSchema;
use pl_protocol::{PureError, Result};
use serde_json::Value;

use super::PureCore;

/// 共享工具 schema 导出选项。
///
/// 该结构只描述模型可见工具目录，不创建 runtime backend。执行路径仍由
/// `ToolSetBuilder` 和显式注册的 backend 决定，因此 git/container/docker
/// 能力可以保持默认关闭。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SharedToolSchemaOptions {
    pub bash: bool,
    pub workspace_files: bool,
    pub ask_user: bool,
    pub subagents: bool,
    pub git: bool,
    pub container: bool,
    pub mcp_resources: bool,
    pub todo: bool,
    pub plan_exit: bool,
}

impl SharedToolSchemaOptions {
    pub fn from_capabilities(capabilities: &ToolCapabilityConfig) -> Self {
        Self {
            bash: capabilities.bash,
            workspace_files: capabilities.workspace_files,
            ask_user: capabilities.ask_user,
            subagents: capabilities.subagents,
            git: capabilities.git,
            container: capabilities.container,
            mcp_resources: capabilities.mcp,
            todo: true,
            plan_exit: true,
        }
    }
}

/// 按能力开关组装 pl-core 的共享工具集合。
#[derive(Debug, Clone, Default)]
pub struct ToolSetBuilder<
    B = LocalExecutionBackend,
    P = NoGitCredentialProvider,
    C = NoContainerBackend,
    A = NoAgentControlBackend,
    M = NoMcpResourceBackend,
    T = NoMcpToolBackend,
> {
    capabilities: ToolCapabilityConfig,
    git_runtime: Option<GitToolRuntime<B, P>>,
    container_runtime: Option<ContainerToolRuntime<C>>,
    agent_control_runtime: Option<AgentControlToolRuntime<A>>,
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
            agent_control_runtime: None,
            mcp_resource_runtime: None,
            mcp_tool_runtime: None,
            allowed_tools: None,
        }
    }
}

impl<B, P, C, A, M, T> ToolSetBuilder<B, P, C, A, M, T> {
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
    ) -> ToolSetBuilder<NB, NP, C, A, M, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: Some(GitToolRuntime {
                config,
                backend,
                credential_provider,
            }),
            container_runtime: self.container_runtime,
            agent_control_runtime: self.agent_control_runtime,
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_container_tools<NC>(self, backend: Arc<NC>) -> ToolSetBuilder<B, P, NC, A, M, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: self.git_runtime,
            container_runtime: Some(ContainerToolRuntime { backend }),
            agent_control_runtime: self.agent_control_runtime,
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_agent_control_tools<NA>(
        self,
        backend: Arc<NA>,
    ) -> ToolSetBuilder<B, P, C, NA, M, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: self.git_runtime,
            container_runtime: self.container_runtime,
            agent_control_runtime: Some(AgentControlToolRuntime { backend }),
            mcp_resource_runtime: self.mcp_resource_runtime,
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_mcp_resource_tools<NM>(
        self,
        backend: Arc<NM>,
    ) -> ToolSetBuilder<B, P, C, A, NM, T> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: self.git_runtime,
            container_runtime: self.container_runtime,
            agent_control_runtime: self.agent_control_runtime,
            mcp_resource_runtime: Some(McpResourceToolRuntime { backend }),
            mcp_tool_runtime: self.mcp_tool_runtime,
            allowed_tools: self.allowed_tools,
        }
    }

    pub fn with_mcp_tools<NT>(
        self,
        schemas: Vec<ToolSchema>,
        backend: Arc<NT>,
    ) -> ToolSetBuilder<B, P, C, A, M, NT> {
        ToolSetBuilder {
            capabilities: self.capabilities,
            git_runtime: self.git_runtime,
            container_runtime: self.container_runtime,
            agent_control_runtime: self.agent_control_runtime,
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

impl<B, P, C, A, M, T> ToolSetBuilder<B, P, C, A, M, T>
where
    B: ExecutionBackend + 'static,
    P: GitCredentialProvider + 'static,
    C: ContainerBackend + 'static,
    A: AgentControlBackend + 'static,
    M: McpResourceBackend + 'static,
    T: McpToolBackend + 'static,
{
    pub async fn register(
        &self,
        core: &mut PureCore,
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
            let (bash_tool, write_stdin_tool) = command_tool_pair(workspace_root.clone());
            register_if_allowed(core, bash_tool, |name| self.tool_allowed(name));
            register_if_allowed(core, write_stdin_tool, |name| self.tool_allowed(name));
        }
        let using_container_workspace =
            self.capabilities.container && self.container_runtime.is_some();
        if self.capabilities.workspace_files && !using_container_workspace {
            register_file_tools(core, |name| self.tool_allowed(name));
        }
        if self.capabilities.workspace_files
            && using_container_workspace
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
        if self.capabilities.subagents {
            if let Some(runtime) = &self.agent_control_runtime {
                register_agent_control_tools(core, runtime.backend.clone(), |name| {
                    self.tool_allowed(name)
                });
            } else {
                register_subagent_tools(core, workspace_instructions.clone(), |name| {
                    self.tool_allowed(name)
                });
            }
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
    if options.subagents {
        schemas.extend(
            AgentControlToolKind::all()
                .iter()
                .copied()
                .map(AgentControlToolKind::to_schema),
        );
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
struct AgentControlToolRuntime<A> {
    backend: Arc<A>,
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
pub struct NoAgentControlBackend;

impl AgentControlBackend for NoAgentControlBackend {
    async fn spawn_agent(
        &self,
        _request: AgentControlSpawnRequest,
    ) -> Result<AgentControlSpawnOutput> {
        Err(missing_backend_error("spawn_agent", "agent control"))
    }

    async fn send_input(
        &self,
        _request: AgentControlSendInputRequest,
    ) -> Result<AgentControlSendInputOutput> {
        Err(missing_backend_error("send_input", "agent control"))
    }

    async fn wait_agent(
        &self,
        _request: AgentControlWaitRequest,
    ) -> Result<AgentControlWaitOutput> {
        Err(missing_backend_error("wait_agent", "agent control"))
    }

    async fn list_agents(
        &self,
        _request: AgentControlListRequest,
    ) -> Result<AgentControlListOutput> {
        Err(missing_backend_error("list_agents", "agent control"))
    }

    async fn close_agent(
        &self,
        _request: AgentControlTargetRequest,
    ) -> Result<AgentControlMessageOutput> {
        Err(missing_backend_error("close_agent", "agent control"))
    }

    async fn resume_agent(
        &self,
        _request: AgentControlTargetRequest,
    ) -> Result<AgentControlMessageOutput> {
        Err(missing_backend_error("resume_agent", "agent control"))
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoMcpResourceBackend;

impl McpResourceBackend for NoMcpResourceBackend {
    async fn list_resources(&self, _request: McpListResourcesRequest) -> Result<Value> {
        Err(missing_backend_error("list_mcp_resources", "MCP resource"))
    }

    async fn list_resource_templates(
        &self,
        _request: McpListResourceTemplatesRequest,
    ) -> Result<Value> {
        Err(missing_backend_error(
            "list_mcp_resource_templates",
            "MCP resource",
        ))
    }

    async fn read_resource(&self, _request: McpReadResourceRequest) -> Result<Value> {
        Err(missing_backend_error("read_mcp_resource", "MCP resource"))
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoMcpToolBackend;

impl McpToolBackend for NoMcpToolBackend {
    async fn call_tool(&self, request: McpToolRequest) -> Result<Value> {
        Err(missing_backend_error(&request.name, "MCP tool"))
    }
}

fn missing_backend_error(tool: &str, backend: &str) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: format!("{backend} backend is not configured"),
    }
}

fn register_if_allowed(
    core: &mut PureCore,
    tool: impl crate::tool::Tool + 'static,
    allowed: impl Fn(&str) -> bool,
) {
    if allowed(tool.name()) {
        core.register_tool(tool);
    }
}

fn register_file_tools(core: &mut PureCore, allowed: impl Fn(&str) -> bool + Copy) {
    register_if_allowed(core, ReadFileTool::new(), allowed);
    register_if_allowed(core, WriteFileTool, allowed);
    register_if_allowed(core, ListFilesTool, allowed);
    register_if_allowed(core, SearchFilesTool, allowed);
    register_if_allowed(core, StatPathTool, allowed);
    register_if_allowed(core, CreateDirectoryTool, allowed);
    register_if_allowed(core, DeletePathTool, allowed);
    register_if_allowed(core, CopyPathTool, allowed);
    register_if_allowed(core, MovePathTool, allowed);
    register_if_allowed(core, ApplyPatchTool, allowed);
}

fn register_container_file_tools<C>(
    core: &mut PureCore,
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

fn register_agent_control_tools<A>(
    core: &mut PureCore,
    backend: Arc<A>,
    allowed: impl Fn(&str) -> bool,
) where
    A: AgentControlBackend + 'static,
{
    for kind in AgentControlToolKind::all() {
        if allowed(kind.name()) {
            core.register_tool(AgentControlTool::new(*kind, backend.clone()));
        }
    }
}

fn register_mcp_resource_tools<M>(
    core: &mut PureCore,
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
    core: &mut PureCore,
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

fn register_subagent_tools(
    core: &mut PureCore,
    workspace_instructions: Option<String>,
    allowed: impl Fn(&str) -> bool + Copy,
) {
    register_if_allowed(
        core,
        SpawnAgentTool::new(
            core.provider.clone(),
            core.reasoning_effort.clone(),
            core.config.clone(),
            core.mcp_runtime.clone(),
            core.lsp_runtime.clone(),
            workspace_instructions.clone(),
        ),
        allowed,
    );
    register_if_allowed(core, WaitAgentTool, allowed);
    register_if_allowed(core, ListAgentsTool, allowed);
    register_if_allowed(
        core,
        SendInputTool::new(
            core.provider.clone(),
            core.reasoning_effort.clone(),
            core.config.clone(),
            core.mcp_runtime.clone(),
            core.lsp_runtime.clone(),
            workspace_instructions,
        ),
        allowed,
    );
    register_if_allowed(core, CloseAgentTool, allowed);
    register_if_allowed(core, ResumeAgentTool, allowed);
}

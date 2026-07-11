use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use pl_protocol::{PureError, Result};
use serde_json::Value;

use crate::tool::{RegisteredTool, Tool, ToolContext, ToolInput, ToolOutput, WorkspaceAccess};
use crate::trace::TraceRecorder;
use crate::turn::{CompileMode, TurnOptions, TurnRequest, TurnResult};

use super::{CoreRuntimeProfile, PureCore, PureCoreBuilder, ToolProfile, ToolSetBuilder};

/// 产品 agent 运行 profile。
///
/// 该类型复用 `CoreRuntimeProfile` 的 workspace、工具能力和运行选项配置，
/// 用于表达宿主产品的定制化 agent，而不是在宿主侧重新实现底层 turn loop。
pub type CoreAgentProfile = CoreRuntimeProfile;

/// pl-core agent runtime kernel。
///
/// 该类型统一持有 `PureCore`、profile 注册出的共享工具和宿主动态工具，
/// 让宿主只配置定制化 agent，不再复刻模型 turn loop 与通用 tool dispatch。
#[derive(Debug)]
pub struct AgentKernel {
    core: PureCore,
}

impl AgentKernel {
    pub fn builder(core_builder: PureCoreBuilder) -> AgentKernelBuilder<NoAgentKernelToolSet> {
        AgentKernelBuilder::new(core_builder)
    }

    pub fn core(&self) -> &PureCore {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut PureCore {
        &mut self.core
    }

    pub fn tool(&self, name: &str) -> Option<&dyn Tool> {
        self.core.tools.get(name)
    }

    pub fn agent_tool_registrar(&self) -> Option<Arc<dyn crate::AgentToolRegistrar>> {
        self.core.agent_tool_registrar.clone()
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.core
            .tools
            .names()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub async fn execute_tool(&self, request: AgentKernelToolRequest) -> Result<ToolOutput> {
        let tool = self
            .tool(&request.name)
            .ok_or_else(|| PureError::ToolExecutionFailed {
                tool: request.name.clone(),
                error: format!("Unknown tool: {}", request.name),
            })?;
        let execution_profile = match &self.core.active_subagent {
            Some(subagent) => {
                crate::TurnExecutionProfile::for_subagent(request.mode, &subagent.role)
            }
            None => crate::TurnExecutionProfile::root(request.mode),
        };
        if !execution_profile.allows_tool(&request.name, tool.effect()) {
            return Err(PureError::ToolExecutionFailed {
                tool: request.name,
                error: "tool is not allowed by the turn execution profile".to_string(),
            });
        }
        let workspace_root = self
            .core
            .workspace_root
            .clone()
            .unwrap_or_else(default_workspace_root);
        let input = ToolInput {
            arguments: request.arguments,
            session_id: request.session_id,
            tool_id: request.tool_id,
            revision_base: request.revision_base,
        };
        let context = ToolContext {
            event_tx: request.event_tx,
            options: request.options,
            workspace_access: request.workspace_access,
            mode: request.mode,
            workspace_root,
            workspace_instructions: self.core.workspace_instructions.clone(),
            instruction_snapshot: request.instruction_snapshot,
            provider_call_id: request.provider_call_id,
            active_subagent: self.core.active_subagent.clone(),
            agent_supervisor: self.core.agent_supervisor.clone(),
            agent_tool_registrar: self.core.agent_tool_registrar.clone(),
            lsp_runtime: self.core.lsp_runtime.clone(),
            parent_session: request.parent_session,
        };
        Tool::execute(tool, input, context).await
    }

    pub fn run_turn<'a>(
        &'a self,
        session: &'a mut crate::CoreSession,
        request: TurnRequest,
        event_tx: pl_trace::AgentEventSender,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        self.core.run_turn(session, request, event_tx)
    }

    pub async fn run_turn_with_options(
        &self,
        session: &mut crate::CoreSession,
        request: TurnRequest,
        event_tx: pl_trace::AgentEventSender,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        self.core
            .run_turn_with_options(session, request, event_tx, options)
            .await
    }

    pub async fn run_turn_with_trace(
        &self,
        session: &mut crate::CoreSession,
        request: TurnRequest,
        recorder: &mut TraceRecorder,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        self.core
            .run_turn_with_trace(session, request, recorder, options)
            .await
    }
}

/// 单次通过 `AgentKernel` 执行工具的请求。
///
/// 产品层用于测试、host facade 或非模型 turn 的单工具调用场景。请求只描述
/// 本次调用的可变输入；workspace、agent registrar、LSP runtime 等稳定上下文
/// 由 kernel 根据当前 profile 和已注册工具集统一注入。
#[derive(Clone)]
pub struct AgentKernelToolRequest {
    name: String,
    arguments: Value,
    session_id: String,
    tool_id: String,
    revision_base: u64,
    event_tx: pl_trace::AgentEventSender,
    options: TurnOptions,
    workspace_access: WorkspaceAccess,
    mode: CompileMode,
    instruction_snapshot: Option<crate::instruction::InstructionSnapshot>,
    provider_call_id: Option<String>,
    parent_session: Arc<crate::CoreSession>,
}

impl AgentKernelToolRequest {
    pub fn new(
        name: impl Into<String>,
        arguments: Value,
        session_id: impl Into<String>,
        tool_id: impl Into<String>,
        event_tx: pl_trace::AgentEventSender,
    ) -> Self {
        Self {
            name: name.into(),
            arguments,
            session_id: session_id.into(),
            tool_id: tool_id.into(),
            revision_base: 0,
            event_tx,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: CompileMode::Simple,
            instruction_snapshot: None,
            provider_call_id: None,
            parent_session: Arc::new(crate::CoreSession::new()),
        }
    }

    pub fn with_revision_base(mut self, revision_base: u64) -> Self {
        self.revision_base = revision_base;
        self
    }

    pub fn with_options(mut self, options: TurnOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_workspace_access(mut self, access: WorkspaceAccess) -> Self {
        self.workspace_access = access;
        self
    }

    pub fn with_mode(mut self, mode: CompileMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_instruction_snapshot(
        mut self,
        snapshot: crate::instruction::InstructionSnapshot,
    ) -> Self {
        self.instruction_snapshot = Some(snapshot);
        self
    }

    pub fn with_provider_call_id(mut self, call_id: impl Into<String>) -> Self {
        self.provider_call_id = Some(call_id.into());
        self
    }

    pub fn with_parent_session(mut self, session: Arc<crate::CoreSession>) -> Self {
        self.parent_session = session;
        self
    }

    pub fn with_parent_history(mut self, history: Vec<pl_protocol::Message>) -> Self {
        self.parent_session = Arc::new(crate::CoreSession::from_messages(history));
        self
    }
}

/// 可随 `AgentKernelBuilder` 注册并由子 agent registrar 重放的工具集合。
///
/// 宿主通常直接使用 `ToolSetBuilder` 实现。自定义实现必须只注册共享 runtime
/// 工具，不应把产品业务工具混入该层；产品工具继续通过 `RegisteredTool`
/// 动态注册。
pub trait AgentKernelToolSet: fmt::Debug + Send + Sync + 'static {
    fn register_tools<'a>(
        &'a self,
        core: &'a mut PureCore,
        workspace_root: PathBuf,
        workspace_instructions: Option<String>,
    ) -> impl Future<Output = ()> + Send + 'a;
}

/// 空工具集合，用作未配置共享工具集时的 `AgentKernelBuilder` 默认状态。
#[derive(Debug, Clone, Default)]
pub struct NoAgentKernelToolSet;

impl AgentKernelToolSet for NoAgentKernelToolSet {
    async fn register_tools(
        &self,
        _core: &mut PureCore,
        _workspace_root: PathBuf,
        _workspace_instructions: Option<String>,
    ) {
    }
}

impl<B, P, C, A, Q, M, T> AgentKernelToolSet for ToolSetBuilder<B, P, C, A, Q, M, T>
where
    B: crate::tool::ExecutionBackend + 'static,
    P: crate::tool::GitCredentialProvider + 'static,
    C: crate::tool::ContainerBackend + 'static,
    A: crate::tool::AgentControlBackend + 'static,
    Q: crate::tool::AgentControlPolicy + 'static,
    M: crate::tool::McpResourceBackend + 'static,
    T: crate::tool::McpToolBackend + 'static,
{
    async fn register_tools(
        &self,
        core: &mut PureCore,
        workspace_root: PathBuf,
        workspace_instructions: Option<String>,
    ) {
        self.register(core, workspace_root, workspace_instructions)
            .await;
    }
}

#[derive(Debug)]
pub struct AgentKernelBuilder<T = NoAgentKernelToolSet> {
    core_builder: PureCoreBuilder,
    profile: CoreAgentProfile,
    tool_set: T,
    runtime_tools: Vec<Arc<dyn Tool>>,
    registered_tools: Vec<RegisteredTool>,
    active_subagent: Option<crate::SubagentContext>,
}

fn default_workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

impl AgentKernelBuilder<NoAgentKernelToolSet> {
    fn new(core_builder: PureCoreBuilder) -> Self {
        Self {
            core_builder,
            profile: CoreAgentProfile::minimal(),
            tool_set: NoAgentKernelToolSet,
            runtime_tools: Vec::new(),
            registered_tools: Vec::new(),
            active_subagent: None,
        }
    }
}

impl<T> AgentKernelBuilder<T> {
    pub fn with_profile(mut self, profile: CoreAgentProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_tool_set<NT>(self, tool_set: NT) -> AgentKernelBuilder<NT>
    where
        NT: AgentKernelToolSet,
    {
        AgentKernelBuilder {
            core_builder: self.core_builder,
            profile: self.profile,
            tool_set,
            runtime_tools: self.runtime_tools,
            registered_tools: self.registered_tools,
            active_subagent: self.active_subagent,
        }
    }

    pub fn with_registered_tool(mut self, tool: RegisteredTool) -> Self {
        self.registered_tools.push(tool);
        self
    }

    /// 将 kernel 配置为指定 child agent 的执行上下文。
    pub fn with_subagent_context(mut self, context: crate::SubagentContext) -> Self {
        self.active_subagent = Some(context);
        self
    }

    pub fn with_tool(mut self, tool: impl Tool + 'static) -> Self {
        self.runtime_tools.push(Arc::new(tool));
        self
    }

    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Arc<dyn Tool>>) -> Self {
        self.runtime_tools.extend(tools);
        self
    }

    pub fn with_registered_tools(
        mut self,
        tools: impl IntoIterator<Item = RegisteredTool>,
    ) -> Self {
        self.registered_tools.extend(tools);
        self
    }
}

impl<T> AgentKernelBuilder<T>
where
    T: AgentKernelToolSet,
{
    pub async fn build(self) -> AgentKernel {
        let workspace_root = self
            .profile
            .workspace_profile
            .root
            .clone()
            .unwrap_or_else(default_workspace_root);
        let workspace_instructions = self.profile.workspace_profile.instructions.clone();
        let mut core = self
            .core_builder
            .with_runtime_profile(self.profile.clone())
            .build();
        if let Some(context) = self.active_subagent {
            core = core.with_subagent_context(context);
        }
        core.register_profile_tools().await;
        self.tool_set
            .register_tools(&mut core, workspace_root, workspace_instructions)
            .await;
        for tool in &self.runtime_tools {
            core.register_tool(tool.clone());
        }
        for tool in &self.registered_tools {
            core.register_tool(tool.clone());
        }
        core.agent_tool_registrar = Some(Arc::new(KernelToolRegistrar {
            profile: self.profile,
            tool_set: self.tool_set,
            runtime_tools: self.runtime_tools,
            registered_tools: self.registered_tools,
        }));
        AgentKernel { core }
    }
}

#[derive(Debug)]
struct KernelToolRegistrar<T> {
    profile: CoreAgentProfile,
    tool_set: T,
    runtime_tools: Vec<Arc<dyn Tool>>,
    registered_tools: Vec<RegisteredTool>,
}

impl<T> crate::AgentToolRegistrar for KernelToolRegistrar<T>
where
    T: AgentKernelToolSet,
{
    fn register_tools<'a>(
        &'a self,
        core: &'a mut PureCore,
        workspace_root: PathBuf,
        workspace_instructions: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            match self.profile.tool_profile {
                ToolProfile::LocalWorkspace => {
                    core.register_default_tools(
                        workspace_root.clone(),
                        workspace_instructions.clone(),
                    )
                    .await;
                }
                ToolProfile::HostProvided | ToolProfile::Minimal => {
                    core.workspace_root = Some(workspace_root.clone());
                    core.workspace_instructions = workspace_instructions.clone();
                }
            }
            self.tool_set
                .register_tools(core, workspace_root.clone(), workspace_instructions.clone())
                .await;
            for tool in &self.runtime_tools {
                core.register_tool(tool.clone());
            }
            for tool in &self.registered_tools {
                core.register_tool(tool.clone());
            }
            Ok(())
        })
    }
}

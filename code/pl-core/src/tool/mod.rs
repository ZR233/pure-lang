mod ask_user;
mod bash;
mod command;
mod container;
mod file;
mod git;
mod lsp;
mod mcp_resource;
mod mcp_tool;
mod multi_agent;
mod output_format;
mod path_policy;
mod plan;
mod shell;
mod skill;
mod text_escape;
mod todo;
mod truncation;
mod workspace_file;

use pl_model::ToolSchema;
use pl_protocol::{PureError, SkillActivation};
use pl_trace::AgentEventSender;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::AgentSupervisor;
use crate::turn::TurnOptions;

pub use ask_user::AskUserTool;
pub(crate) use bash::command_tool_pair;
pub use bash::{BashInput, BashTool, WriteStdinTool};
#[cfg(feature = "docker-tools")]
pub use container::DockerCliContainerBackend;
pub use container::{
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecOutput,
    ContainerExecRequest, ContainerTool, ContainerToolExecution, ContainerToolKind,
    NoContainerBackend, TOOL_CONTAINER_COPY, TOOL_CONTAINER_EXEC, execute_container_tool,
};
pub use file::{
    CopyPathTool, CreateDirectoryTool, DeletePathTool, MovePathTool, StatPathTool, WriteFileTool,
};
pub use git::{
    ExecutionBackend, ExecutionOutput, ExecutionRequest, GIT_TOKEN_ENV, GitCredential,
    GitCredentialOperation, GitCredentialProvider, GitCredentialRequest, GitPolicy,
    GitShellCommandRequest, GitShellCredential, GitTool, GitToolKind, GitWorkspaceConfig,
    LocalExecutionBackend, NoGitCredentialProvider, TOOL_GIT_BRANCH, TOOL_GIT_COMMIT,
    TOOL_GIT_DIFF, TOOL_GIT_FETCH, TOOL_GIT_PUSH, TOOL_GIT_STATUS, TOOL_GIT_SYNC_DEFAULT_BRANCH,
    TOOL_GIT_WORKSPACE_INFO, git_askpass_script, git_shell_command, git_shell_credential_prelude,
    git_shell_retry_function,
};
pub use lsp::{LspLanguageTool, LspQueryTool, lsp_tool_for_language};
pub use mcp_resource::{
    McpListResourceTemplatesRequest, McpListResourcesRequest, McpReadResourceRequest,
    McpResourceBackend, McpResourceTool, McpResourceToolKind, TOOL_LIST_MCP_RESOURCE_TEMPLATES,
    TOOL_LIST_MCP_RESOURCES, TOOL_READ_MCP_RESOURCE,
};
pub use mcp_tool::{
    HostMcpToolSpec, McpTool, McpToolBackend, McpToolRequest, host_mcp_tool_schema,
    host_mcp_tool_schemas,
};
pub use multi_agent::{
    AgentControlAgentRecord, AgentControlAgentType, AgentControlAgentTypePolicy,
    AgentControlBackend, AgentControlListOutput, AgentControlListRequest,
    AgentControlMessageOutput, AgentControlPolicy, AgentControlSendInputOutput,
    AgentControlSendInputRequest, AgentControlSpawnOutput, AgentControlSpawnRequest,
    AgentControlStatusKind, AgentControlTargetRequest, AgentControlTool, AgentControlToolKind,
    AgentControlWaitOutput, AgentControlWaitRequest, AllowAllAgentControlPolicy, CloseAgentTool,
    ListAgentsTool, ResumeAgentTool, SendInputTool, SpawnAgentTool, TOOL_CLOSE_AGENT,
    TOOL_LIST_AGENTS, TOOL_RESUME_AGENT, TOOL_SEND_INPUT, TOOL_SPAWN_AGENT, TOOL_WAIT_AGENT,
    WaitAgentTool,
};
pub use output_format::{
    DEFAULT_MODEL_TOOL_OUTPUT_TOKENS, SECRET_REDACTION_REPLACEMENT, SecretRedaction,
    ToolHistoryProjection, ToolLifecyclePhase, ToolLifecycleProjection,
    ToolOutputArtifactDescriptor, ToolOutputArtifactPathRequest, ToolOutputCapture,
    ToolOutputCaptureRequest, ToolOutputStream, ToolOutputStreamCapture, ToolOutputStreamSizes,
    model_visible_tool_output, model_visible_tool_output_with_tokens, redacted_trace_preview_value,
    tool_history_projection, tool_lifecycle_projection, tool_lifecycle_projections,
    tool_output_artifact_file_path, trace_preview_output, trace_preview_value,
};
pub(crate) use path_policy::{PathAccess, ToolPathPolicy};
pub use plan::PlanExitTool;
pub use shell::{ShellCommandTimeout, shell_command_with_timeout, shell_quote_word};
pub use skill::{SkillManageTool, SkillViewTool, SkillsListTool};
pub use todo::{TOOL_UPDATE_TODO_LIST, TodoListTool};
pub use truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};
pub use workspace_file::{
    ContainerWorkspaceFileBackend, LocalWorkspaceFileBackend, LocalWorkspaceFileTool,
    TOOL_APPLY_PATCH, TOOL_LIST_FILES, TOOL_READ_FILE, TOOL_SEARCH_FILES, WorkspaceFileBackend,
    WorkspaceFileListEntry, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadRequest, WorkspaceFileRemoveRequest, WorkspaceFileSearchMatch,
    WorkspaceFileSearchRequest, WorkspaceFileSearchResult, WorkspaceFileStat,
    WorkspaceFileStatRequest, WorkspaceFileTool, WorkspaceFileToolExecution, WorkspaceFileToolKind,
    WorkspaceFileWriteRequest, execute_workspace_file_tool,
};

/// 便捷类型别名：boxed future。
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type RegisteredToolFuture = Pin<Box<dyn Future<Output = Result<ToolOutput, PureError>> + Send>>;
type RegisteredToolHandler = dyn Fn(ToolInput, ToolContext) -> RegisteredToolFuture + Send + Sync;

/// 严格 object 输入 schema 中的字段。
///
/// 产品层和共享工具都应通过 `required` / `optional` 命名构造器声明字段，
/// 避免在不同仓库里重复维护 `required` 数组和 `additionalProperties` 形状。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInputSchemaField {
    name: String,
    schema: serde_json::Value,
    required: bool,
}

impl ToolInputSchemaField {
    pub fn required(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            schema,
            required: true,
        }
    }

    pub fn optional(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            schema,
            required: false,
        }
    }
}

/// 构造工具统一使用的严格 object 输入 schema。
pub fn strict_tool_input_schema(
    fields: impl IntoIterator<Item = ToolInputSchemaField>,
) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for field in fields {
        if field.required {
            required.push(serde_json::Value::String(field.name.clone()));
        }
        properties.insert(field.name, field.schema);
    }
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// 构造 function tool schema，并统一使用严格 object 输入 schema。
pub fn function_tool_schema(
    name: impl Into<String>,
    description: impl Into<String>,
    fields: impl IntoIterator<Item = ToolInputSchemaField>,
) -> ToolSchema {
    ToolSchema::function(name, description, strict_tool_input_schema(fields))
}

/// 动态注册工具 schema 不符合 pl-core typed handler 入口时的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredToolSchemaError {
    name: String,
}

impl RegisteredToolSchemaError {
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for RegisteredToolSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "registered tool `{}` must use a function schema",
            self.name
        )
    }
}

impl std::error::Error for RegisteredToolSchemaError {}

/// 等待宿主工具后端 future，并统一响应 turn cancellation。
///
/// 宿主 adapter 仍负责业务调用和错误类型；pl-core 负责维护工具执行过程中
/// cancellation token 与后台 future 的竞争语义，避免每个产品后端重复手写
/// `tokio::select!`。
pub async fn run_tool_backend_with_cancellation<F, T, E>(
    future: F,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
    cancelled_error: impl FnOnce() -> E,
) -> std::result::Result<T, E>
where
    F: Future<Output = std::result::Result<T, E>> + Send,
{
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err(cancelled_error());
        }
        return tokio::select! {
            result = future => result,
            _ = token.cancelled() => Err(cancelled_error()),
        };
    }

    future.await
}

/// 工具执行抽象（dyn-compatible）。
///
/// `execute` 返回 `BoxFuture` 以支持 trait object。
/// `ToolContext` 提供事件转发、审批策略和当前 subagent 运行边界。
/// 具体实现中可用 `Box::pin(async move { ... })` 包裹异步逻辑。
pub trait Tool: fmt::Debug + Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }
    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        if self.supports_parallel_tool_calls() {
            ToolRuntimeLockPolicy::Shared
        } else {
            ToolRuntimeLockPolicy::Exclusive
        }
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>>;

    fn to_schema(&self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

impl<T> Tool for Arc<T>
where
    T: Tool + ?Sized + 'static,
{
    fn name(&self) -> &str {
        (**self).name()
    }

    fn description(&self) -> &str {
        (**self).description()
    }

    fn input_schema(&self) -> serde_json::Value {
        (**self).input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        (**self).supports_parallel_tool_calls()
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        (**self).runtime_lock_policy()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        (**self).execute(input, context)
    }
}

/// Runtime coordination policy for tools within one model response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRuntimeLockPolicy {
    Exclusive,
    Shared,
    None,
}

/// 单次工具执行上下文。
///
/// 由核心 turn 循环注入，工具通过它访问事件流、审批策略、
/// workspace 信息，以及当前 subagent 的父子关系。
#[derive(Clone)]
pub struct ToolContext {
    pub event_tx: AgentEventSender,
    pub options: TurnOptions,
    pub workspace_access: WorkspaceAccess,
    pub mode: crate::turn::CompileMode,
    pub workspace_root: PathBuf,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<crate::instruction::InstructionSnapshot>,
    pub provider_call_id: Option<String>,
    pub active_subagent: Option<SubagentContext>,
    pub agent_supervisor: AgentSupervisor,
    pub agent_tool_registrar: Option<Arc<dyn crate::AgentToolRegistrar>>,
    pub lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
    pub parent_session: Arc<crate::session::CoreSession>,
}

/// 单次工具调用的路径访问策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceAccess {
    #[default]
    WorkspaceOnly,
    ExternalAllowed,
}

impl WorkspaceAccess {
    pub fn allows_external(self) -> bool {
        matches!(self, Self::ExternalAllowed)
    }
}

/// 当前工具调用所在的 subagent 运行边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentContext {
    pub id: String,
    pub parent_id: Option<String>,
    pub agent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub depth: u32,
}

impl fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolContext")
            .field("workspace_root", &self.workspace_root)
            .field("permission_mode", &self.options.permission_mode)
            .field("workspace_access", &self.workspace_access)
            .field("provider_call_id", &self.provider_call_id)
            .field("active_subagent", &self.active_subagent)
            .field("lsp_runtime", &self.lsp_runtime.is_some())
            .finish_non_exhaustive()
    }
}

impl ToolContext {
    pub(crate) fn allows_workspace_escape(&self) -> bool {
        self.options.permission_mode.allows_workspace_escape()
            || self.workspace_access.allows_external()
    }

    pub(crate) async fn workspace_write_lock(&self) -> WorkspaceWriteGuard {
        workspace_write_locks().lock_for(&self.workspace_root).await
    }
}

type WorkspaceWriteGuard = OwnedMutexGuard<()>;

#[derive(Default)]
struct WorkspaceWriteLocks {
    locks: std::sync::Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl WorkspaceWriteLocks {
    async fn lock_for(&self, workspace_root: &std::path::Path) -> WorkspaceWriteGuard {
        let key =
            std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
        let lock = {
            let mut locks = self.locks.lock().unwrap_or_else(|poisoned| {
                tracing::warn!("workspace write lock was poisoned, recovering");
                poisoned.into_inner()
            });
            locks
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }
}

fn workspace_write_locks() -> &'static WorkspaceWriteLocks {
    static LOCKS: OnceLock<WorkspaceWriteLocks> = OnceLock::new();
    LOCKS.get_or_init(WorkspaceWriteLocks::default)
}

/// 工具注册表。
///
/// 管理已注册的工具实例，提供按名称查找和 schema 收集能力。
#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.tools.iter().map(|t| t.name()).collect();
        f.debug_struct("ToolRegistry")
            .field("tools", &names)
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        assert!(
            self.get(tool.name()).is_none(),
            "duplicate tool name: {}",
            tool.name()
        );
        self.tools.push(Box::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| &**t)
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.iter().map(|t| t.to_schema()).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// 移除指定名称的工具（用于动态卸载）。
    pub fn unregister(&mut self, name: &str) -> bool {
        let len_before = self.tools.len();
        self.tools.retain(|tool| tool.name() != name);
        self.tools.len() != len_before
    }

    /// 注册当前可用的语言 LSP 工具。
    ///
    /// 遍历 `available_languages()` 返回的语言列表，为每个语言注册一个
    /// `LspLanguageTool`。同时移除之前注册但已不再可用的语言工具。
    pub async fn register_lsp_languages(
        &mut self,
        registry: &pl_lsp::LspRuntimeRegistry,
    ) -> Vec<String> {
        let available = registry.available_languages().await;
        self.sync_lsp_language_tools(registry, available)
    }

    fn sync_lsp_language_tools(
        &mut self,
        registry: &pl_lsp::LspRuntimeRegistry,
        available: Vec<pl_lsp::LanguageToolInfo>,
    ) -> Vec<String> {
        let tool_names: Vec<String> = available
            .iter()
            .map(|info| format!("lsp_query_{}", info.language_id))
            .collect();
        self.tools.retain(|tool| {
            let name = tool.name();
            if name.starts_with("lsp_query_") {
                tool_names.iter().any(|tn| tn == name)
            } else {
                true
            }
        });
        let mut registered = Vec::new();
        for info in &available {
            let lang_id = &info.language_id;
            let tool_name = format!("lsp_query_{lang_id}");
            if self.get(&tool_name).is_none() {
                self.tools
                    .push(lsp_tool_for_language(info, registry.clone()));
            }
            if !registered.contains(&info.language_id) {
                registered.push(info.language_id.clone());
            }
        }
        registered
    }
}

/// 通用工具输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInput {
    pub arguments: serde_json::Value,
    pub session_id: String,
    pub tool_id: String,
    #[serde(default)]
    pub revision_base: u64,
}

/// 通用工具输出。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    pub description: String,
    pub truncated: OutputTruncation,
    pub output_file: PathBuf,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_events: Vec<ToolRuntimeEvent>,
}

/// 根据产品工具的模型可见输出构造 pl-core 工具输出。
///
/// 产品工具 handler 仍负责业务执行和输出文本生成；pl-core 统一把成功状态和
/// 结束回合语义映射成 canonical `ToolOutput`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputModelOutputRequest {
    pub model_output: String,
    pub success: bool,
    pub ends_turn: bool,
}

/// 工具执行结果的通用中间形态。
///
/// handler 可以保留完整输出和 artifact 元数据，同时由 pl-core 统一生成模型可见
/// 输出、成功状态和结束回合事件，避免产品层各自维护一套截断和 `ToolOutput`
/// 映射规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionResult<Artifact = serde_json::Value> {
    pub success: bool,
    pub output: String,
    pub model_output: String,
    pub ends_turn: bool,
    pub output_artifacts: Vec<Artifact>,
}

impl<Artifact> ToolExecutionResult<Artifact> {
    pub fn json(value: impl Serialize) -> Result<Self, PureError> {
        let output =
            serde_json::to_string(&value).map_err(|error| PureError::ToolExecutionFailed {
                tool: "registered_tool".to_string(),
                error: format!("failed to serialize JSON output: {error}"),
            })?;
        Ok(Self::success(output))
    }

    pub fn success(output: impl Into<String>) -> Self {
        Self::new(true, output.into(), false)
    }

    pub fn failure(output: impl Into<String>) -> Self {
        Self::new(false, output.into(), false)
    }

    pub fn new(success: bool, output: String, ends_turn: bool) -> Self {
        Self::with_model_tokens(
            success,
            output,
            ends_turn,
            DEFAULT_MODEL_TOOL_OUTPUT_TOKENS,
            Vec::new(),
        )
    }

    pub fn with_model_tokens(
        success: bool,
        output: String,
        ends_turn: bool,
        max_output_tokens: usize,
        output_artifacts: Vec<Artifact>,
    ) -> Self {
        let model_output = model_visible_tool_output_with_tokens(&output, max_output_tokens);
        Self::with_model_output(success, output, model_output, ends_turn, output_artifacts)
    }

    pub fn with_model_output(
        success: bool,
        output: String,
        model_output: String,
        ends_turn: bool,
        output_artifacts: Vec<Artifact>,
    ) -> Self {
        Self {
            success,
            output,
            model_output,
            ends_turn,
            output_artifacts,
        }
    }

    pub fn into_tool_output(self) -> ToolOutput {
        ToolOutput::from_model_output(ToolOutputModelOutputRequest {
            model_output: self.model_output,
            success: self.success,
            ends_turn: self.ends_turn,
        })
    }
}

impl ToolOutput {
    pub fn from_model_output(request: ToolOutputModelOutputRequest) -> Self {
        Self {
            description: request.model_output,
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: if request.success { Some(0) } else { Some(1) },
            timed_out: false,
            runtime_events: if request.ends_turn {
                vec![ToolRuntimeEvent::EndTurn]
            } else {
                Vec::new()
            },
        }
    }

    pub fn json(value: impl Serialize) -> Result<Self, PureError> {
        let description =
            serde_json::to_string(&value).map_err(|error| PureError::ToolExecutionFailed {
                tool: "registered_tool".to_string(),
                error: format!("failed to serialize JSON output: {error}"),
            })?;
        Ok(Self {
            description,
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: None,
            timed_out: false,
            runtime_events: Vec::new(),
        })
    }

    /// 消费工具输出并返回模型可见文本。
    ///
    /// `ToolOutput` 内部目前用 `description` 存储模型可见输出；产品层应通过该
    /// 语义方法读取，避免把字段名当作共享协议。
    pub fn into_model_output(self) -> String {
        self.description
    }

    /// 从工具运行时事件中提取并解码输出 artifact。
    ///
    /// `OutputArtifacts` 是 pl-core 的工具执行事件细节，产品层应通过这个方法
    /// 取得自身协议需要的 artifact 类型，而不是直接匹配 `ToolRuntimeEvent`。
    /// 无法解码的条目会被忽略，和生命周期投影的 artifact 容错语义保持一致。
    pub fn output_artifacts_as<T>(&self) -> Vec<T>
    where
        T: DeserializeOwned,
    {
        self.runtime_events
            .iter()
            .filter_map(|event| match event {
                ToolRuntimeEvent::OutputArtifacts { artifacts } => Some(artifacts.as_slice()),
                ToolRuntimeEvent::SkillActivated {
                    activation: _activation,
                } => None,
                ToolRuntimeEvent::ToolResultRevision {
                    revision: _revision,
                } => None,
                ToolRuntimeEvent::EndTurn => None,
            })
            .flatten()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect()
    }

    /// 判断工具输出是否要求当前 turn 结束。
    ///
    /// 结束回合是 pl-core 工具运行时事件的一种语义；产品层应调用该方法，而不是
    /// 直接匹配 `ToolRuntimeEvent::EndTurn`。
    pub fn ends_turn(&self) -> bool {
        self.runtime_events.iter().any(|event| match event {
            ToolRuntimeEvent::EndTurn => true,
            ToolRuntimeEvent::SkillActivated {
                activation: _activation,
            } => false,
            ToolRuntimeEvent::ToolResultRevision {
                revision: _revision,
            } => false,
            ToolRuntimeEvent::OutputArtifacts {
                artifacts: _artifacts,
            } => false,
        })
    }

    /// 将 canonical 工具输出投影回产品层常用的执行结果形态。
    ///
    /// 该方法统一 `ToolOutput` 的成功判定、模型可见输出、结束回合语义和 artifact
    /// 解码，避免产品 adapter 直接读取 `exit_code`、`description` 或运行时事件。
    pub fn to_execution_result<T>(&self) -> ToolExecutionResult<T>
    where
        T: DeserializeOwned,
    {
        ToolExecutionResult::with_model_output(
            self.exit_code.unwrap_or(0) == 0,
            self.description.clone(),
            self.description.clone(),
            self.ends_turn(),
            self.output_artifacts_as(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ToolRuntimeEvent {
    SkillActivated { activation: SkillActivation },
    ToolResultRevision { revision: u64 },
    OutputArtifacts { artifacts: Vec<serde_json::Value> },
    EndTurn,
}

/// 运行时动态注册的工具。
///
/// 宿主产品用它把自身业务 handler 挂入 pl-core 的统一 registry 和 dispatch；
/// handler 只负责业务副作用，工具生命周期、trace、权限和 tool result history
/// 仍由 pl-core 统一处理。
#[derive(Clone)]
pub struct RegisteredTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    supports_parallel_tool_calls: bool,
    runtime_lock_policy: Option<ToolRuntimeLockPolicy>,
    handler: Arc<RegisteredToolHandler>,
}

impl fmt::Debug for RegisteredTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredTool")
            .field("name", &self.name)
            .field(
                "supports_parallel_tool_calls",
                &self.supports_parallel_tool_calls,
            )
            .field("runtime_lock_policy", &self.runtime_lock_policy)
            .finish_non_exhaustive()
    }
}

impl RegisteredTool {
    pub fn new<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        F: Fn(ToolInput, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolOutput, PureError>> + Send + 'static,
    {
        let name = name.into();
        let tool_name = name.clone();
        Self {
            name,
            description: description.into(),
            input_schema,
            supports_parallel_tool_calls: false,
            runtime_lock_policy: None,
            handler: Arc::new(move |input, context| {
                if context
                    .options
                    .cancellation_token
                    .as_ref()
                    .is_some_and(|token| token.is_cancelled())
                {
                    let tool = tool_name.clone();
                    return Box::pin(async move {
                        Err(PureError::ToolExecutionFailed {
                            tool,
                            error: "tool execution cancelled".to_string(),
                        })
                    }) as RegisteredToolFuture;
                }
                Box::pin(handler(input, context)) as RegisteredToolFuture
            }),
        }
    }

    pub fn from_execution_result<F, Fut, Artifact>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        F: Fn(ToolInput, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolExecutionResult<Artifact>, PureError>> + Send + 'static,
        Artifact: Send + 'static,
    {
        Self::new(name, description, input_schema, move |input, context| {
            let future = handler(input, context);
            async move { future.await.map(ToolExecutionResult::into_tool_output) }
        })
    }

    pub fn from_fallible_execution_result<F, Fut, Artifact, Error>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        F: Fn(ToolInput, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<ToolExecutionResult<Artifact>, Error>>
            + Send
            + 'static,
        Artifact: Send + 'static,
        Error: fmt::Display + Send + 'static,
    {
        let name = name.into();
        let tool_name = name.clone();
        Self::new(name, description, input_schema, move |input, context| {
            let future = handler(input, context);
            let tool_name = tool_name.clone();
            async move {
                future
                    .await
                    .map(ToolExecutionResult::into_tool_output)
                    .map_err(|error| PureError::ToolExecutionFailed {
                        tool: tool_name,
                        error: error.to_string(),
                    })
            }
        })
    }

    /// 从模型可见 function schema 注册带强类型输入的产品工具。
    ///
    /// 产品层只需传入自己已经声明的 `ToolSchema` 和业务 handler；pl-core 统一
    /// 解包 function schema 的 name/description/input schema，并复用 typed
    /// 输入解析、错误映射和 `ToolExecutionResult` 输出投影。
    pub fn from_schema_typed_fallible_execution_result<Input, F, Fut, Artifact, Error>(
        schema: ToolSchema,
        handler: F,
    ) -> std::result::Result<Self, RegisteredToolSchemaError>
    where
        Input: DeserializeOwned + Send + 'static,
        F: Fn(Input, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<ToolExecutionResult<Artifact>, Error>>
            + Send
            + 'static,
        Artifact: Send + 'static,
        Error: fmt::Display + Send + 'static,
    {
        match schema {
            ToolSchema::Function {
                name,
                description,
                input_schema,
            } => Ok(Self::from_typed_fallible_execution_result(
                name,
                description,
                input_schema,
                handler,
            )),
            ToolSchema::Custom {
                name,
                description,
                format,
            } => {
                let _ = (description, format);
                Err(RegisteredToolSchemaError { name })
            }
        }
    }

    /// 注册带强类型输入的产品工具。
    ///
    /// 宿主只提供产品输入类型和业务 handler；`pl-core` 负责把模型传入的
    /// JSON arguments 反序列化为该类型，并把输入解析错误、业务错误和
    /// `ToolExecutionResult` 统一映射成 canonical `ToolOutput`。
    pub fn from_typed_fallible_execution_result<Input, F, Fut, Artifact, Error>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        Input: DeserializeOwned + Send + 'static,
        F: Fn(Input, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<ToolExecutionResult<Artifact>, Error>>
            + Send
            + 'static,
        Artifact: Send + 'static,
        Error: fmt::Display + Send + 'static,
    {
        let name = name.into();
        let tool_name = name.clone();
        Self::new(name, description, input_schema, move |input, context| {
            let tool_name = tool_name.clone();
            let arguments = match serde_json::from_value::<Input>(input.arguments) {
                Ok(arguments) => arguments,
                Err(error) => {
                    return Box::pin(async move {
                        Err(PureError::ToolExecutionFailed {
                            tool: tool_name,
                            error: format!("invalid input: {error}"),
                        })
                    }) as RegisteredToolFuture;
                }
            };
            let future = handler(arguments, context);
            Box::pin(async move {
                future
                    .await
                    .map(ToolExecutionResult::into_tool_output)
                    .map_err(|error| PureError::ToolExecutionFailed {
                        tool: tool_name,
                        error: error.to_string(),
                    })
            }) as RegisteredToolFuture
        })
    }

    pub fn with_parallel_tool_calls(mut self) -> Self {
        self.supports_parallel_tool_calls = true;
        self
    }

    pub fn with_runtime_lock_policy(mut self, policy: ToolRuntimeLockPolicy) -> Self {
        self.runtime_lock_policy = Some(policy);
        self
    }
}

impl Tool for RegisteredTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.supports_parallel_tool_calls
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        self.runtime_lock_policy.unwrap_or({
            if self.supports_parallel_tool_calls {
                ToolRuntimeLockPolicy::Shared
            } else {
                ToolRuntimeLockPolicy::Exclusive
            }
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        (self.handler)(input, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn empty_truncation() -> OutputTruncation {
        OutputTruncation::empty()
    }

    #[derive(Debug)]
    struct EchoTool;

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo input"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "text": { "type": "string" } }
            })
        }

        fn execute<'a>(
            &'a self,
            _input: ToolInput,
            _context: ToolContext,
        ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
            Box::pin(async {
                Ok(ToolOutput {
                    description: "ok".to_string(),
                    truncated: empty_truncation(),
                    output_file: PathBuf::new(),
                    exit_code: None,
                    timed_out: false,
                    runtime_events: Vec::new(),
                })
            })
        }
    }

    #[test]
    fn registry_register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
        assert!(reg.get("echo").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn registry_schemas() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        let schemas = reg.schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name(), "echo");
    }

    #[test]
    fn registry_is_empty_initially() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn tool_output_from_model_output_sets_exit_code_and_end_turn_event() {
        let output = ToolOutput::from_model_output(ToolOutputModelOutputRequest {
            model_output: "saved".to_string(),
            success: false,
            ends_turn: true,
        });

        assert_eq!(
            output,
            ToolOutput {
                description: "saved".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(1),
                timed_out: false,
                runtime_events: vec![ToolRuntimeEvent::EndTurn],
            }
        );
    }

    #[test]
    fn tool_output_consumes_model_visible_output() {
        let output = ToolOutput::from_model_output(ToolOutputModelOutputRequest {
            model_output: "visible".to_string(),
            success: true,
            ends_turn: false,
        });

        assert_eq!(output.into_model_output(), "visible");
    }

    #[test]
    fn tool_output_reports_end_turn_runtime_event() {
        let output = ToolOutput {
            description: "saved".to_string(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(0),
            timed_out: false,
            runtime_events: vec![ToolRuntimeEvent::ToolResultRevision { revision: 1 }],
        };
        assert!(!output.ends_turn());

        let output = ToolOutput {
            runtime_events: vec![
                ToolRuntimeEvent::ToolResultRevision { revision: 1 },
                ToolRuntimeEvent::EndTurn,
            ],
            ..output
        };
        assert!(output.ends_turn());
    }

    #[test]
    fn tool_output_decodes_runtime_output_artifacts() {
        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct ArtifactRecord {
            id: String,
        }

        let output = ToolOutput {
            description: "saved".to_string(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(0),
            timed_out: false,
            runtime_events: vec![
                ToolRuntimeEvent::ToolResultRevision { revision: 1 },
                ToolRuntimeEvent::OutputArtifacts {
                    artifacts: vec![serde_json::json!({"id": "artifact-1"})],
                },
                ToolRuntimeEvent::EndTurn,
            ],
        };

        assert_eq!(
            output.output_artifacts_as::<ArtifactRecord>(),
            vec![ArtifactRecord {
                id: "artifact-1".to_string(),
            }]
        );
    }

    #[test]
    fn tool_output_projects_execution_result_for_product_adapters() {
        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct ArtifactRecord {
            id: String,
        }

        let output = ToolOutput {
            description: "model output".to_string(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(1),
            timed_out: false,
            runtime_events: vec![
                ToolRuntimeEvent::OutputArtifacts {
                    artifacts: vec![serde_json::json!({"id": "artifact-1"})],
                },
                ToolRuntimeEvent::EndTurn,
            ],
        };

        assert_eq!(
            output.to_execution_result::<ArtifactRecord>(),
            ToolExecutionResult {
                success: false,
                output: "model output".to_string(),
                model_output: "model output".to_string(),
                ends_turn: true,
                output_artifacts: vec![ArtifactRecord {
                    id: "artifact-1".to_string(),
                }],
            }
        );
    }

    #[test]
    fn tool_execution_result_keeps_full_output_and_builds_tool_output() {
        let execution = ToolExecutionResult::with_model_tokens(
            true,
            "full output".to_string(),
            true,
            10_000,
            vec![serde_json::json!({"id": "artifact-1"})],
        );

        assert_eq!(
            execution,
            ToolExecutionResult {
                success: true,
                output: "full output".to_string(),
                model_output: "full output".to_string(),
                ends_turn: true,
                output_artifacts: vec![serde_json::json!({"id": "artifact-1"})],
            }
        );
        assert_eq!(
            execution.into_tool_output(),
            ToolOutput {
                description: "full output".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: vec![ToolRuntimeEvent::EndTurn],
            }
        );
    }

    #[test]
    fn tool_execution_result_serializes_json_model_output() {
        let execution = ToolExecutionResult::<serde_json::Value>::json(serde_json::json!({
            "queued": [1],
            "deduped": [2],
            "ignored": []
        }))
        .expect("serialize JSON tool output");

        assert_eq!(
            execution,
            ToolExecutionResult {
                success: true,
                output: "{\"deduped\":[2],\"ignored\":[],\"queued\":[1]}".to_string(),
                model_output: "{\"deduped\":[2],\"ignored\":[],\"queued\":[1]}".to_string(),
                ends_turn: false,
                output_artifacts: Vec::new(),
            }
        );
    }

    #[test]
    fn tool_execution_result_exposes_explicit_success_and_failure_constructors() {
        assert_eq!(
            ToolExecutionResult::<serde_json::Value>::success("ok"),
            ToolExecutionResult::new(true, "ok".to_string(), false)
        );
        assert_eq!(
            ToolExecutionResult::<serde_json::Value>::failure("bad"),
            ToolExecutionResult::new(false, "bad".to_string(), false)
        );
    }

    #[test]
    fn function_tool_schema_builds_strict_object_input_schema() {
        let schema = function_tool_schema(
            "save_task_plan",
            "Save a task plan.",
            [
                ToolInputSchemaField::required("title", serde_json::json!({ "type": "string" })),
                ToolInputSchemaField::required("markdown", serde_json::json!({ "type": "string" })),
                ToolInputSchemaField::optional("metadata", serde_json::json!({ "type": "object" })),
            ],
        );

        let ToolSchema::Function {
            name,
            description,
            input_schema,
        } = schema
        else {
            panic!("function tool schema");
        };
        assert_eq!(name, "save_task_plan");
        assert_eq!(description, "Save a task plan.");
        assert_eq!(
            input_schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "markdown": { "type": "string" },
                    "metadata": { "type": "object" }
                },
                "required": ["title", "markdown"],
                "additionalProperties": false
            })
        );
    }

    #[test]
    fn strict_tool_input_schema_uses_named_field_constructors() {
        let schema = strict_tool_input_schema([
            ToolInputSchemaField::required("path", serde_json::json!({ "type": "string" })),
            ToolInputSchemaField::optional("name", serde_json::json!({ "type": "string" })),
        ]);

        assert_eq!(
            schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "name": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
            })
        );
    }

    #[tokio::test]
    async fn registered_tool_from_execution_result_honors_cancelled_context() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tool_calls = calls.clone();
        let tool = RegisteredTool::from_execution_result(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            move |_input, _context| {
                let tool_calls = tool_calls.clone();
                async move {
                    tool_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(ToolExecutionResult::<serde_json::Value>::new(
                        true,
                        "ran".to_string(),
                        false,
                    ))
                }
            },
        );
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                ToolContext {
                    event_tx,
                    options: TurnOptions::default().with_cancellation(token),
                    workspace_access: WorkspaceAccess::WorkspaceOnly,
                    mode: crate::turn::CompileMode::Simple,
                    workspace_root: PathBuf::new(),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    agent_supervisor: AgentSupervisor::default(),
                    agent_tool_registrar: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::CoreSession::new()),
                },
            )
            .await;

        assert!(matches!(
            result,
            Err(PureError::ToolExecutionFailed { tool, error })
                if tool == "product_tool" && error.contains("cancel")
        ));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn registered_tool_from_fallible_execution_result_maps_display_error() {
        #[derive(Debug)]
        struct DisplayError(&'static str);

        impl fmt::Display for DisplayError {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.0)
            }
        }

        let tool = RegisteredTool::from_fallible_execution_result(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            |_input, _context| async move {
                Err::<ToolExecutionResult<serde_json::Value>, DisplayError>(DisplayError("boom"))
            },
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                ToolContext {
                    event_tx,
                    options: TurnOptions::default(),
                    workspace_access: WorkspaceAccess::WorkspaceOnly,
                    mode: crate::turn::CompileMode::Simple,
                    workspace_root: PathBuf::new(),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    agent_supervisor: AgentSupervisor::default(),
                    agent_tool_registrar: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::CoreSession::new()),
                },
            )
            .await;

        assert!(matches!(
            result,
            Err(PureError::ToolExecutionFailed { tool, error })
                if tool == "product_tool" && error == "boom"
        ));
    }

    #[tokio::test]
    async fn registered_tool_from_typed_fallible_execution_result_deserializes_input() {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct ProductInput {
            item_id: String,
        }

        let tool = RegisteredTool::from_typed_fallible_execution_result(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            |input: ProductInput, _context| async move {
                Ok::<_, &'static str>(
                    ToolExecutionResult::<serde_json::Value>::json(serde_json::json!({
                        "itemId": input.item_id
                    }))
                    .expect("json output"),
                )
            },
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let output = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({ "itemId": "task-1" }),
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                ToolContext {
                    event_tx,
                    options: TurnOptions::default(),
                    workspace_access: WorkspaceAccess::WorkspaceOnly,
                    mode: crate::turn::CompileMode::Simple,
                    workspace_root: PathBuf::new(),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    agent_supervisor: AgentSupervisor::default(),
                    agent_tool_registrar: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::CoreSession::new()),
                },
            )
            .await
            .expect("typed product tool output");

        assert_eq!(output.description, "{\"itemId\":\"task-1\"}");
    }

    #[test]
    fn registered_tool_from_schema_uses_function_schema_metadata() {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct ProductInput {
            _item_id: String,
        }

        let schema = function_tool_schema(
            "product_tool",
            "Product tool",
            [ToolInputSchemaField::required(
                "itemId",
                serde_json::json!({ "type": "string" }),
            )],
        );

        let tool = RegisteredTool::from_schema_typed_fallible_execution_result(
            schema,
            |_input: ProductInput, _context| async move {
                Ok::<_, &'static str>(ToolExecutionResult::<serde_json::Value>::success("ok"))
            },
        )
        .expect("function schema");

        assert_eq!(tool.name(), "product_tool");
        assert_eq!(tool.description(), "Product tool");
        assert_eq!(
            tool.input_schema(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "itemId": { "type": "string" }
                },
                "required": ["itemId"],
                "additionalProperties": false,
            })
        );
    }

    #[test]
    fn registered_tool_from_schema_rejects_custom_schema() {
        #[derive(Debug, Deserialize)]
        struct ProductInput;

        let result = RegisteredTool::from_schema_typed_fallible_execution_result(
            ToolSchema::custom_grammar("custom_tool", "Custom tool", "lark", "start: /x/"),
            |_input: ProductInput, _context| async move {
                Ok::<_, &'static str>(ToolExecutionResult::<serde_json::Value>::success("ok"))
            },
        );

        assert_eq!(
            result
                .expect_err("custom schema must be rejected")
                .to_string(),
            "registered tool `custom_tool` must use a function schema"
        );
    }

    #[tokio::test]
    async fn registered_tool_from_typed_fallible_execution_result_rejects_invalid_input() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ProductInput {
            #[serde(rename = "itemId")]
            _item_id: String,
        }

        let tool = RegisteredTool::from_typed_fallible_execution_result(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            |_input: ProductInput, _context| async move {
                Ok::<_, &'static str>(ToolExecutionResult::<serde_json::Value>::success("ok"))
            },
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({ "item_id": "task-1" }),
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                ToolContext {
                    event_tx,
                    options: TurnOptions::default(),
                    workspace_access: WorkspaceAccess::WorkspaceOnly,
                    mode: crate::turn::CompileMode::Simple,
                    workspace_root: PathBuf::new(),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    agent_supervisor: AgentSupervisor::default(),
                    agent_tool_registrar: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::CoreSession::new()),
                },
            )
            .await;

        assert!(matches!(
            result,
            Err(PureError::ToolExecutionFailed { tool, error })
                if tool == "product_tool"
                    && error.contains("invalid input")
                    && error.contains("itemId")
        ));
    }

    #[tokio::test]
    async fn tool_backend_future_returns_cancelled_error_before_running() {
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();

        let result = run_tool_backend_with_cancellation(
            async { Ok::<_, &'static str>("ran") },
            Some(token),
            || "cancelled",
        )
        .await;

        assert_eq!(result, Err("cancelled"));
    }

    #[tokio::test]
    async fn tool_backend_future_returns_cancelled_error_while_running() {
        let token = tokio_util::sync::CancellationToken::new();
        let task_token = token.clone();
        let task = tokio::spawn(async move {
            run_tool_backend_with_cancellation(
                async {
                    std::future::pending::<()>().await;
                    Ok::<_, &'static str>("ran")
                },
                Some(task_token),
                || "cancelled",
            )
            .await
        });

        token.cancel();

        assert_eq!(task.await.expect("task joins"), Err("cancelled"));
    }

    #[test]
    fn registry_debug_shows_names() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        let debug = format!("{reg:?}");
        assert!(debug.contains("echo"));
    }

    #[test]
    fn registry_unregister_removes_named_tool() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        assert!(reg.unregister("echo"));
        assert!(!reg.unregister("echo"));
        assert!(reg.get("echo").is_none());
    }

    #[test]
    fn model_visible_tool_output_truncates_json_with_codex_shape() {
        let long_stdout = "x".repeat(65);
        let output = model_visible_tool_output_with_tokens(
            &serde_json::json!({ "status": 0, "stdout": long_stdout, "stderr": "" }).to_string(),
            8,
        );
        let value = serde_json::from_str::<serde_json::Value>(&output).unwrap();

        assert_eq!(value["truncated"], true);
        assert!(value.pointer("/bytesReturned").is_some());
        assert!(value.pointer("/bytesOmitted").is_some());
        assert!(value.pointer("/nextOffset").is_some());
        assert!(value.pointer("/bytes_returned").is_none());
        let visible = value
            .get("stdout")
            .or_else(|| value.get("jsonPreview"))
            .and_then(serde_json::Value::as_str)
            .expect("visible output");
        assert!(visible.len() <= 32);
    }

    #[test]
    fn trace_preview_redacts_sensitive_values() {
        let value = serde_json::json!({
            "token": "secret-token",
            "nested": { "api_key": "secret-key", "normal": "visible" },
            "payload": "YWJj".repeat(90),
        });
        let preview = trace_preview_value(&value, 1_000);

        assert!(preview.contains("<redacted>"));
        assert!(preview.contains("visible"));
        assert!(!preview.contains("secret-token"));
        assert!(!preview.contains("secret-key"));
        assert!(!preview.contains(&"YWJj".repeat(30)));
    }

    #[test]
    fn explicit_secret_redaction_handles_text_and_json() {
        let redaction = SecretRedaction::new(["secret", "secret-token", ""]);

        assert_eq!(
            redaction.redact_str("secret-token and secret"),
            "<redacted> and <redacted>"
        );
        assert_eq!(
            redaction.redact_json_value(serde_json::json!({
                "secret-token": "visible",
                "items": ["secret-token", { "value": "secret" }],
            })),
            serde_json::json!({
                "<redacted>": "visible",
                "items": ["<redacted>", { "value": "<redacted>" }],
            })
        );
    }

    #[test]
    fn registry_sync_lsp_language_tools_registers_and_removes_languages() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        let registry = pl_lsp::LspRuntimeRegistry::new();
        let rust = pl_lsp::LanguageToolInfo {
            language_id: "rust".to_string(),
            server_id: "rust-analyzer".to_string(),
            display_name: "rust-analyzer".to_string(),
            extensions: vec![".rs".to_string()],
        };

        let registered = reg.sync_lsp_language_tools(&registry, vec![rust]);

        assert_eq!(registered, vec!["rust".to_string()]);
        assert!(reg.get("echo").is_some());
        assert!(reg.get("lsp_query_rust").is_some());

        let rust = pl_lsp::LanguageToolInfo {
            language_id: "rust".to_string(),
            server_id: "rust-analyzer".to_string(),
            display_name: "rust-analyzer".to_string(),
            extensions: vec![".rs".to_string()],
        };
        let registered = reg.sync_lsp_language_tools(&registry, vec![rust]);

        assert_eq!(registered, vec!["rust".to_string()]);
        assert!(reg.get("lsp_query_rust").is_some());

        let registered = reg.sync_lsp_language_tools(&registry, Vec::new());

        assert!(registered.is_empty());
        assert!(reg.get("echo").is_some());
        assert!(reg.get("lsp_query_rust").is_none());
    }

    #[tokio::test]
    async fn workspace_write_lock_is_shared_for_same_workspace() {
        let root = std::env::temp_dir().join(format!(
            "pure-lang-write-lock-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let context = ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: crate::turn::CompileMode::Simple,
            workspace_root: root.clone(),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            agent_supervisor: AgentSupervisor::default(),
            agent_tool_registrar: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::session::CoreSession::new()),
        };
        let first_guard = context.workspace_write_lock().await;
        let second_context = context.clone();
        let second = tokio::spawn(async move { second_context.workspace_write_lock().await });
        tokio::task::yield_now().await;

        assert!(!second.is_finished());
        drop(first_guard);
        let second_guard = second.await.unwrap();
        drop(second_guard);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}

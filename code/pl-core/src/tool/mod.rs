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
mod skill;
mod text_escape;
mod todo;
mod truncation;
mod workspace_file;

use pl_model::ToolSchema;
use pl_protocol::{PureError, SkillActivation};
use pl_trace::AgentEventSender;
use serde::{Deserialize, Serialize};
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
    ApplyPatchTool, CopyPathTool, CreateDirectoryTool, DeletePathTool, ListFilesTool, MovePathTool,
    ReadFileTool, SearchFilesTool, StatPathTool, WriteFileTool,
};
pub use git::{
    ExecutionBackend, ExecutionOutput, ExecutionRequest, GIT_TOKEN_ENV, GitCredential,
    GitCredentialOperation, GitCredentialProvider, GitCredentialRequest, GitPolicy, GitTool,
    GitToolKind, GitWorkspaceConfig, LocalExecutionBackend, NoGitCredentialProvider,
    TOOL_GIT_BRANCH, TOOL_GIT_COMMIT, TOOL_GIT_DIFF, TOOL_GIT_FETCH, TOOL_GIT_PUSH,
    TOOL_GIT_STATUS, TOOL_GIT_SYNC_DEFAULT_BRANCH, TOOL_GIT_WORKSPACE_INFO,
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
    AgentControlAgentRecord, AgentControlBackend, AgentControlListOutput, AgentControlListRequest,
    AgentControlMessageOutput, AgentControlSendInputOutput, AgentControlSendInputRequest,
    AgentControlSpawnOutput, AgentControlSpawnRequest, AgentControlTargetRequest, AgentControlTool,
    AgentControlToolKind, AgentControlWaitOutput, AgentControlWaitRequest, CloseAgentTool,
    ListAgentsTool, ResumeAgentTool, SendInputTool, SpawnAgentTool, TOOL_CLOSE_AGENT,
    TOOL_LIST_AGENTS, TOOL_RESUME_AGENT, TOOL_SEND_INPUT, TOOL_SPAWN_AGENT, TOOL_WAIT_AGENT,
    WaitAgentTool,
};
pub use output_format::{
    DEFAULT_MODEL_TOOL_OUTPUT_TOKENS, ToolHistoryProjection, ToolLifecyclePhase,
    ToolLifecycleProjection, ToolOutputArtifactDescriptor, ToolOutputArtifactPathRequest,
    ToolOutputCapture, ToolOutputCaptureRequest, ToolOutputStream, ToolOutputStreamCapture,
    ToolOutputStreamSizes, model_visible_tool_output, model_visible_tool_output_with_tokens,
    redacted_trace_preview_value, tool_history_projection, tool_lifecycle_projection,
    tool_lifecycle_projections, tool_output_artifact_file_path, trace_preview_output,
    trace_preview_value,
};
pub(crate) use path_policy::{PathAccess, ToolPathPolicy};
pub use plan::PlanExitTool;
pub use skill::{SkillManageTool, SkillViewTool, SkillsListTool};
pub use todo::{TOOL_UPDATE_TODO_LIST, TodoListTool};
pub use truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};
pub use workspace_file::{
    ContainerWorkspaceFileBackend, LocalWorkspaceFileBackend, TOOL_APPLY_PATCH, TOOL_LIST_FILES,
    TOOL_READ_FILE, TOOL_SEARCH_FILES, WorkspaceFileBackend, WorkspaceFileListEntry,
    WorkspaceFileListRequest, WorkspaceFileListResult, WorkspaceFileReadRequest,
    WorkspaceFileRemoveRequest, WorkspaceFileSearchMatch, WorkspaceFileSearchRequest,
    WorkspaceFileSearchResult, WorkspaceFileStat, WorkspaceFileStatRequest, WorkspaceFileTool,
    WorkspaceFileToolExecution, WorkspaceFileToolKind, WorkspaceFileWriteRequest,
    execute_workspace_file_tool,
};

/// 便捷类型别名：boxed future。
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type RegisteredToolFuture = Pin<Box<dyn Future<Output = Result<ToolOutput, PureError>> + Send>>;
type RegisteredToolHandler = dyn Fn(ToolInput, ToolContext) -> RegisteredToolFuture + Send + Sync;

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
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            supports_parallel_tool_calls: false,
            runtime_lock_policy: None,
            handler: Arc::new(move |input, context| Box::pin(handler(input, context))),
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
            mode: crate::turn::CompileMode::Auto,
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

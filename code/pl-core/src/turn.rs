use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::{AbortHandle, AbortRegistration, Abortable};
use pl_model::TokenUsage;
use pl_protocol::{BudgetLimitKind, BudgetUsage, MessageContent};
use pl_trace::{TraceAttachment, TraceEvent};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::context_compaction::ContextCompactionSnapshot;
use crate::instruction::InstructionSnapshot;

/// Agent 树结构限制常量。
pub const AGENT_MAX_COUNT: usize = 16;
pub const AGENT_MAX_DEPTH: u32 = 3;
/// 默认 wall-clock 安全上限（30 分钟），参考 Codex 的 agent_job_max_runtime_seconds。
pub const DEFAULT_WALL_CLOCK_MS: u64 = 1_800_000;

/// 单轮 wall-clock 安全预算。
///
/// 参考 Codex 的设计：不限制 model step / tool call 迭代次数，
/// 让模型自己决定何时完成（通过返回无 tool call 的 content-only 响应）。
/// 仅保留 wall-clock 作为防止无限运行的安全兜底。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnBudget {
    /// Wall-clock 安全上限（毫秒）。超时后 turn 将被终止。
    pub wall_clock_ms: u64,
}

impl TurnBudget {
    pub fn new(wall_clock_ms: u64) -> Self {
        Self { wall_clock_ms }
    }

    pub fn root_default() -> Self {
        Self {
            wall_clock_ms: DEFAULT_WALL_CLOCK_MS,
        }
    }

    pub fn child_default() -> Self {
        Self {
            wall_clock_ms: DEFAULT_WALL_CLOCK_MS,
        }
    }
}

impl Default for TurnBudget {
    fn default() -> Self {
        Self::root_default()
    }
}

/// Agent tree 结构限制。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentBudget {
    pub max_agents: usize,
    pub max_depth: u32,
}

impl Default for AgentBudget {
    fn default() -> Self {
        Self {
            max_agents: AGENT_MAX_COUNT,
            max_depth: AGENT_MAX_DEPTH,
        }
    }
}

/// Turn 与 agent 协作的配置策略。
///
/// 参考 Codex：不使用 step-based 预算，仅保留 wall-clock 和 agent 结构限制。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetPolicy {
    pub agent_budget: AgentBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BudgetLimit {
    pub kind: BudgetLimitKind,
    pub usage: BudgetUsage,
}

/// 运行时用量追踪器。
///
/// 仅追踪 wall-clock 安全上限；model step / tool call 仅做可观测性计数，不强制。
#[derive(Debug, Clone)]
pub(crate) struct BudgetTracker {
    wall_clock_ms: u64,
    usage: BudgetUsage,
    started_at: std::time::Instant,
}

impl BudgetTracker {
    pub fn new(budget: TurnBudget) -> Self {
        Self {
            wall_clock_ms: budget.wall_clock_ms,
            usage: BudgetUsage::default(),
            started_at: std::time::Instant::now(),
        }
    }

    pub fn usage(&self) -> BudgetUsage {
        let mut usage = self.usage;
        usage.elapsed_ms = self.started_at.elapsed().as_millis() as u64;
        usage
    }

    /// 记录一次模型推理（仅追踪，不限制）。
    pub fn record_model_step(&mut self) {
        self.usage.model_steps += 1;
    }

    /// 记录一次工具调用（仅追踪，不限制）。
    pub fn record_tool_call(&mut self, tool_name: &str) {
        if tool_name == "wait_agent" {
            self.usage.wait_calls += 1;
        } else {
            self.usage.tool_calls += 1;
        }
    }

    /// 检查 wall-clock 安全上限。
    pub fn check_wall_clock(&self) -> std::result::Result<(), BudgetLimit> {
        let usage = self.usage();
        if usage.elapsed_ms > self.wall_clock_ms {
            return Err(BudgetLimit {
                kind: BudgetLimitKind::WallClock,
                usage,
            });
        }
        Ok(())
    }
}

/// 编译请求的执行模式。
///
/// `Plan` 产出规划与解释，也可以在已注册工具边界内做只读探索；
/// `Auto` 允许模型生成更主动的编译步骤和子任务。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    Plan,
    #[default]
    Auto,
}

impl CompileMode {
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Plan => include_str!("../prompts/plan.md"),
            Self::Auto => include_str!("../prompts/auto.md"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Auto => "auto",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "plan" => Self::Plan,
            "auto" => Self::Auto,
            _ => Self::Auto,
        }
    }
}

/// 单轮核心编译请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRequest {
    pub turn_id: Option<String>,
    pub prompt: String,
    pub user_content: MessageContent,
    pub mode: CompileMode,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<InstructionSnapshot>,
    pub budget: TurnBudget,
    pub materialized_attachments: Vec<crate::MaterializedAttachment>,
    pub trace_attachments: Vec<TraceAttachment>,
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>, mode: CompileMode) -> Self {
        let prompt = prompt.into();
        Self {
            turn_id: None,
            user_content: MessageContent::Text(prompt.clone()),
            prompt,
            mode,
            workspace_instructions: None,
            instruction_snapshot: None,
            budget: TurnBudget::root_default(),
            materialized_attachments: Vec::new(),
            trace_attachments: Vec::new(),
        }
    }

    pub fn with_user_content(mut self, content: MessageContent) -> Self {
        self.user_content = content;
        self
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    pub fn with_materialized_attachments(
        mut self,
        attachments: Vec<crate::MaterializedAttachment>,
    ) -> Self {
        self.materialized_attachments = attachments;
        self
    }

    pub fn with_trace_attachments(mut self, attachments: Vec<TraceAttachment>) -> Self {
        self.trace_attachments = attachments;
        self
    }

    pub fn with_workspace_instructions(mut self, instructions: String) -> Self {
        self.workspace_instructions = Some(instructions);
        self
    }

    pub fn with_instruction_snapshot(mut self, snapshot: InstructionSnapshot) -> Self {
        self.instruction_snapshot = Some(snapshot);
        self
    }

    pub fn with_budget(mut self, budget: TurnBudget) -> Self {
        self.budget = budget;
        self
    }
}

pub type InteractionFuture =
    Pin<Box<dyn Future<Output = pl_protocol::InteractionResolution> + Send>>;
pub type InteractionCallback =
    Arc<dyn Fn(pl_protocol::InteractionRequest) -> InteractionFuture + Send + Sync>;

/// 会话级权限模式。
///
/// Pure v1 只实现本地策略层，不提供 OS 沙箱。该模式决定 workspace 外访问
/// 是请求用户审批、请求 reviewer 审批，还是直接放行。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    #[default]
    #[serde(alias = "workspace-write")]
    RequestApproval,
    AutoReview,
    FullAccess,
}

impl PermissionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::RequestApproval => "request-approval",
            Self::AutoReview => "auto-review",
            Self::FullAccess => "full-access",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "request-approval" => Self::RequestApproval,
            "auto-review" => Self::AutoReview,
            "workspace-write" => Self::RequestApproval,
            "full-access" => Self::FullAccess,
            _ => Self::RequestApproval,
        }
    }

    pub fn allows_workspace_escape(self) -> bool {
        matches!(self, Self::FullAccess)
    }

    pub fn is_default(&self) -> bool {
        matches!(self, Self::RequestApproval)
    }
}

/// 工具审批策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolApprovalPolicy {
    #[default]
    AutoAllow,
    Manual,
    DenyAll,
}

/// 单次工具调用审批请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequest {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
}

/// 单次工具调用审批结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolApprovalDecision {
    Approved,
    Denied { reason: String },
}

/// 模型工具调用与本地工具执行的并行策略。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolExecutionMode {
    #[default]
    ModelDefault,
    Sequential,
    Parallel,
}

/// `request_user_input` 工具的 turn 生命周期模式。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UserInputMode {
    /// 等待宿主返回用户回答，并把回答作为工具结果写回模型历史。
    #[default]
    AwaitResponse,
    /// 发出用户交互事件后结束当前 turn，交由宿主在后续输入中继续。
    EmitAndEndTurn,
}

/// 单轮运行选项。
///
/// 用于前端控制工具审批等运行时行为。默认值允许 workspace 内工具直接执行，
/// workspace 外访问按权限模式请求审批。
#[derive(Clone)]
pub struct TurnOptions {
    pub tool_approval_policy: ToolApprovalPolicy,
    pub permission_mode: PermissionMode,
    pub interaction_callback: Option<InteractionCallback>,
    pub cancellation_token: Option<CancellationToken>,
    pub tool_execution_mode: ToolExecutionMode,
    pub prompt_cache_key: Option<String>,
    pub user_input_mode: UserInputMode,
}

impl TurnOptions {
    pub fn new(tool_approval_policy: ToolApprovalPolicy) -> Self {
        Self {
            tool_approval_policy,
            permission_mode: match tool_approval_policy {
                ToolApprovalPolicy::AutoAllow => PermissionMode::RequestApproval,
                ToolApprovalPolicy::Manual => PermissionMode::RequestApproval,
                ToolApprovalPolicy::DenyAll => PermissionMode::RequestApproval,
            },
            interaction_callback: None,
            cancellation_token: None,
            tool_execution_mode: ToolExecutionMode::ModelDefault,
            prompt_cache_key: None,
            user_input_mode: UserInputMode::AwaitResponse,
        }
    }

    pub fn deny_all() -> Self {
        Self::new(ToolApprovalPolicy::DenyAll)
    }

    pub fn with_cancellation(mut self, cancellation_token: CancellationToken) -> Self {
        self.cancellation_token = Some(cancellation_token);
        self
    }

    pub fn with_tool_execution_mode(mut self, tool_execution_mode: ToolExecutionMode) -> Self {
        self.tool_execution_mode = tool_execution_mode;
        self
    }

    pub fn with_prompt_cache_key(mut self, prompt_cache_key: impl Into<String>) -> Self {
        self.prompt_cache_key = Some(prompt_cache_key.into());
        self
    }

    pub fn with_user_input_mode(mut self, user_input_mode: UserInputMode) -> Self {
        self.user_input_mode = user_input_mode;
        self
    }

    pub fn with_user_input_end_turn(self) -> Self {
        self.with_user_input_mode(UserInputMode::EmitAndEndTurn)
    }

    pub fn with_interaction_callback(mut self, callback: InteractionCallback) -> Self {
        self.interaction_callback = Some(callback);
        self
    }

    pub fn with_permission_mode(mut self, permission_mode: PermissionMode) -> Self {
        self.permission_mode = permission_mode;
        self
    }

    pub fn requires_user_approval_callback(&self) -> bool {
        matches!(self.permission_mode, PermissionMode::RequestApproval)
            || matches!(self.tool_approval_policy, ToolApprovalPolicy::Manual)
    }
}

impl Default for TurnOptions {
    fn default() -> Self {
        Self::new(ToolApprovalPolicy::AutoAllow)
    }
}

impl std::fmt::Debug for TurnOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnOptions")
            .field("tool_approval_policy", &self.tool_approval_policy)
            .field("permission_mode", &self.permission_mode)
            .field(
                "interaction_callback",
                &self.interaction_callback.as_ref().map(|_| "<callback>"),
            )
            .field(
                "cancellation_token",
                &self.cancellation_token.as_ref().map(|_| "<token>"),
            )
            .field("tool_execution_mode", &self.tool_execution_mode)
            .field("prompt_cache_key", &self.prompt_cache_key)
            .field("user_input_mode", &self.user_input_mode)
            .finish()
    }
}

/// 宿主可复用的单轮异步任务控制句柄。
///
/// 该类型只负责 turn task 的取消 token 与 abort 生命周期，不写入任何宿主状态。
/// 产品层仍负责自己的持久化、事件投影、排队和最终状态转换。
#[derive(Clone)]
pub struct TurnTaskHandle {
    cancellation_token: CancellationToken,
    abort_handle: Option<AbortHandle>,
}

impl TurnTaskHandle {
    /// 启动一个可取消、可 abort 的 turn task，并把同一个取消 token 传给任务体。
    pub fn spawn_with_token<F, Fut>(task: F) -> Self
    where
        F: FnOnce(CancellationToken) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (handle, abort_registration) = Self::new_abortable();
        let task_token = handle.cancellation_token();
        tokio::spawn(Abortable::new(task(task_token), abort_registration));
        handle
    }

    /// 包装由宿主或测试外部管理的取消 token。
    ///
    /// 返回的句柄没有 abort 能力，调用 `abort` 或 `cancel_and_abort_after` 只会取消 token。
    pub fn from_external_token(cancellation_token: CancellationToken) -> Self {
        Self {
            cancellation_token,
            abort_handle: None,
        }
    }

    /// 返回传给 turn task 的取消 token。
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// 判断当前 turn task 是否已经收到取消信号。
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    /// 只发送合作式取消信号，不强制 abort task。
    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    /// 立即 abort 由该句柄启动的 task。
    pub fn abort(&self) {
        if let Some(abort_handle) = self.abort_handle.clone() {
            abort_handle.abort();
        }
    }

    /// 先发送取消信号，等待 grace 后仍处于取消态则 abort task。
    pub fn cancel_and_abort_after(&self, grace: Duration) {
        self.cancel();
        let Some(abort_handle) = self.abort_handle.clone() else {
            return;
        };
        let token = self.cancellation_token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            if token.is_cancelled() {
                abort_handle.abort();
            }
        });
    }

    fn new_abortable() -> (Self, AbortRegistration) {
        let cancellation_token = CancellationToken::new();
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        (
            Self {
                cancellation_token,
                abort_handle: Some(abort_handle),
            },
            abort_registration,
        )
    }
}

/// 活动 turn 的宿主无关控制记录。
///
/// 该结构保存宿主自己的 turn/session 标识，同时把 task 取消与 abort 操作统一
/// 委托给 pl-core 的 `TurnTaskHandle`。宿主可以继续用自己的持久化模型记录状态，
/// 但不再需要复制 task 生命周期控制字段。
#[derive(Clone)]
pub struct ActiveTurnControl<TurnId, SessionId> {
    pub turn_id: TurnId,
    pub session_id: SessionId,
    task_handle: TurnTaskHandle,
}

impl<TurnId, SessionId> ActiveTurnControl<TurnId, SessionId> {
    pub fn new(turn_id: TurnId, session_id: SessionId, task_handle: TurnTaskHandle) -> Self {
        Self {
            turn_id,
            session_id,
            task_handle,
        }
    }

    /// 判断底层 turn task 是否已经收到取消信号。
    pub fn is_task_cancelled(&self) -> bool {
        self.task_handle.is_cancelled()
    }

    /// 只取消底层 turn task，不强制 abort。
    pub fn cancel_task(&self) {
        self.task_handle.cancel();
    }

    /// 立即 abort 底层 turn task。
    pub fn abort_task(&self) {
        self.task_handle.abort();
    }

    /// 先取消底层 turn task，等待 grace 后仍处于取消态则强制 abort。
    pub fn cancel_task_and_abort_after(&self, grace: Duration) {
        self.task_handle.cancel_and_abort_after(grace);
    }
}

/// 活动 turn 控制记录的并发 slot。
///
/// 宿主使用该类型保存“当前活动 turn”时，可以复用 pl-core 的匹配、读取和清理
/// 语义，而不必在产品层重复维护 `Mutex<Option<...>>`。
#[derive(Clone)]
pub struct ActiveTurnSlot<TurnId, SessionId> {
    inner: Arc<Mutex<Option<ActiveTurnControl<TurnId, SessionId>>>>,
}

impl<TurnId, SessionId> ActiveTurnSlot<TurnId, SessionId> {
    /// 创建一个空的活动 turn slot。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// 创建一个已安装活动 turn 的 slot。
    pub fn with_active(control: ActiveTurnControl<TurnId, SessionId>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(control))),
        }
    }

    /// 安装新的活动 turn，返回被替换的旧记录。
    pub fn set(
        &self,
        control: ActiveTurnControl<TurnId, SessionId>,
    ) -> Option<ActiveTurnControl<TurnId, SessionId>> {
        self.inner
            .lock()
            .expect("active turn slot lock")
            .replace(control)
    }

    /// 清空活动 turn，返回原记录。
    pub fn clear(&self) -> Option<ActiveTurnControl<TurnId, SessionId>> {
        self.inner.lock().expect("active turn slot lock").take()
    }
}

impl<TurnId, SessionId> Default for ActiveTurnSlot<TurnId, SessionId> {
    fn default() -> Self {
        Self::new()
    }
}

impl<TurnId, SessionId> ActiveTurnSlot<TurnId, SessionId>
where
    TurnId: Clone,
    SessionId: Clone,
{
    /// 读取当前活动 turn。
    pub fn current(&self) -> Option<ActiveTurnControl<TurnId, SessionId>> {
        self.inner.lock().expect("active turn slot lock").clone()
    }
}

impl<TurnId, SessionId> ActiveTurnSlot<TurnId, SessionId>
where
    TurnId: Clone + PartialEq,
    SessionId: Clone,
{
    /// 仅当 turn id 匹配时取出并清空活动 turn。
    pub fn take_if_turn(&self, turn_id: &TurnId) -> Option<ActiveTurnControl<TurnId, SessionId>> {
        let mut current = self.inner.lock().expect("active turn slot lock");
        if current
            .as_ref()
            .is_some_and(|control| control.turn_id == *turn_id)
        {
            current.take()
        } else {
            None
        }
    }
}

/// 启动 agent turn 的宿主无关状态转换输入。
///
/// pl-core 只定义“允许开始时应写入哪些通用字段”；具体状态枚举、
/// turn id 和时间戳类型仍由产品层提供。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnStartTransition<TurnId, Status, Timestamp> {
    turn_id: TurnId,
    running_status: Status,
    updated_at: Timestamp,
}

impl<TurnId, Status, Timestamp> AgentTurnStartTransition<TurnId, Status, Timestamp> {
    pub fn new(turn_id: TurnId, running_status: Status, updated_at: Timestamp) -> Self {
        Self {
            turn_id,
            running_status,
            updated_at,
        }
    }

    /// 根据产品层的可启动判断生成状态变更。
    pub fn evaluate(self, can_start: bool) -> AgentTurnStartOutcome<Status, TurnId, Timestamp> {
        if can_start {
            AgentTurnStartOutcome::Started(AgentTurnStartMutation {
                status: self.running_status,
                current_turn: Some(self.turn_id),
                updated_at: self.updated_at,
                last_error: None,
                cancel_requested: false,
            })
        } else {
            AgentTurnStartOutcome::Busy
        }
    }
}

/// 启动 agent turn 后应写入宿主状态的通用字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnStartMutation<Status, TurnId, Timestamp> {
    pub status: Status,
    pub current_turn: Option<TurnId>,
    pub updated_at: Timestamp,
    pub last_error: Option<String>,
    pub cancel_requested: bool,
}

/// 启动 agent turn 的通用状态转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnStartOutcome<Status, TurnId, Timestamp> {
    Started(AgentTurnStartMutation<Status, TurnId, Timestamp>),
    Busy,
}

/// 完成 agent turn 的宿主无关状态转换输入。
///
/// pl-core 只判断即将完成的 turn 是否仍是当前 turn，并生成清空
/// `current_turn`、写入终态和更新时间的通用 mutation。产品层继续负责
/// 持久化、事件投影和错误展示策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnCompletionTransition<TurnId, Status, Timestamp> {
    turn_id: TurnId,
    terminal_status: Status,
    updated_at: Timestamp,
    error: Option<String>,
}

impl<TurnId, Status, Timestamp> AgentTurnCompletionTransition<TurnId, Status, Timestamp> {
    pub fn new(
        turn_id: TurnId,
        terminal_status: Status,
        updated_at: Timestamp,
        error: Option<String>,
    ) -> Self {
        Self {
            turn_id,
            terminal_status,
            updated_at,
            error,
        }
    }
}

impl<TurnId, Status, Timestamp> AgentTurnCompletionTransition<TurnId, Status, Timestamp>
where
    TurnId: PartialEq,
{
    /// 根据当前宿主记录中的 turn id 生成完成状态变更。
    pub fn evaluate(
        self,
        current_turn: Option<&TurnId>,
    ) -> AgentTurnCompletionOutcome<Status, TurnId, Timestamp> {
        if current_turn.is_some_and(|current| *current == self.turn_id) {
            AgentTurnCompletionOutcome::Completed(AgentTurnCompletionMutation {
                status: self.terminal_status,
                current_turn: None,
                updated_at: self.updated_at,
                last_error: self.error,
            })
        } else {
            AgentTurnCompletionOutcome::Stale
        }
    }
}

/// 完成 agent turn 后应写入宿主状态的通用字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnCompletionMutation<Status, TurnId, Timestamp> {
    pub status: Status,
    pub current_turn: Option<TurnId>,
    pub updated_at: Timestamp,
    pub last_error: Option<String>,
}

/// 完成 agent turn 的通用状态转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnCompletionOutcome<Status, TurnId, Timestamp> {
    Completed(AgentTurnCompletionMutation<Status, TurnId, Timestamp>),
    Stale,
}

/// turn 内状态更新是否必须绑定当前 turn。
///
/// 宿主在进入模型、工具等待等阶段时通常需要校验当前 turn，恢复或非严格
/// 投影路径则可以显式选择允许旧 turn 写入普通状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTurnStatusGuard {
    EnforceCurrent,
    AllowStale,
}

impl AgentTurnStatusGuard {
    fn enforces_current(self) -> bool {
        matches!(self, Self::EnforceCurrent)
    }
}

/// turn 内部阶段状态更新的宿主无关状态转换输入。
///
/// pl-core 统一处理“请求已取消”和“必须匹配当前 turn”两类通用判断；
/// 宿主只需要传入自己的状态枚举、turn id 和时间戳类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnStatusTransition<TurnId, Status, Timestamp> {
    turn_id: TurnId,
    status: Status,
    updated_at: Timestamp,
    guard: AgentTurnStatusGuard,
}

impl<TurnId, Status, Timestamp> AgentTurnStatusTransition<TurnId, Status, Timestamp> {
    pub fn new(
        turn_id: TurnId,
        status: Status,
        updated_at: Timestamp,
        guard: AgentTurnStatusGuard,
    ) -> Self {
        Self {
            turn_id,
            status,
            updated_at,
            guard,
        }
    }
}

impl<TurnId, Status, Timestamp> AgentTurnStatusTransition<TurnId, Status, Timestamp>
where
    TurnId: PartialEq,
{
    /// 根据取消状态和当前 turn id 生成状态变更。
    pub fn evaluate(
        self,
        cancelled: bool,
        current_turn: Option<&TurnId>,
    ) -> AgentTurnStatusOutcome<Status, Timestamp> {
        if cancelled
            || (self.guard.enforces_current()
                && current_turn.is_none_or(|current| *current != self.turn_id))
        {
            AgentTurnStatusOutcome::Cancelled
        } else {
            AgentTurnStatusOutcome::Applied(AgentTurnStatusMutation {
                status: self.status,
                updated_at: self.updated_at,
            })
        }
    }

    /// 根据取消 token 和当前 turn id 生成状态变更。
    pub fn evaluate_with_token(
        self,
        cancellation_token: &CancellationToken,
        current_turn: Option<&TurnId>,
    ) -> AgentTurnStatusOutcome<Status, Timestamp> {
        self.evaluate(cancellation_token.is_cancelled(), current_turn)
    }
}

/// turn 内状态更新后应写入宿主状态的通用字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnStatusMutation<Status, Timestamp> {
    pub status: Status,
    pub updated_at: Timestamp,
}

/// turn 内状态更新的通用状态转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnStatusOutcome<Status, Timestamp> {
    Applied(AgentTurnStatusMutation<Status, Timestamp>),
    Cancelled,
}

/// 长流程步骤中的当前 turn 保护。
///
/// 容器、MCP 和其它耗时准备阶段可在副作用前后复用该 guard，确保当前
/// 操作仍属于同一个 agent turn。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnCurrentGuard<TurnId> {
    turn_id: TurnId,
}

impl<TurnId> AgentTurnCurrentGuard<TurnId> {
    pub fn new(turn_id: TurnId) -> Self {
        Self { turn_id }
    }
}

impl<TurnId> AgentTurnCurrentGuard<TurnId>
where
    TurnId: PartialEq,
{
    /// 根据取消状态和宿主当前 turn id 判断流程是否还能继续。
    pub fn evaluate(
        &self,
        cancelled: bool,
        current_turn: Option<&TurnId>,
    ) -> AgentTurnCurrentOutcome {
        if cancelled || current_turn.is_none_or(|current| *current != self.turn_id) {
            AgentTurnCurrentOutcome::Interrupted
        } else {
            AgentTurnCurrentOutcome::Current
        }
    }

    /// 根据取消 token 和宿主当前 turn id 判断流程是否还能继续。
    pub fn evaluate_with_token(
        &self,
        cancellation_token: &CancellationToken,
        current_turn: Option<&TurnId>,
    ) -> AgentTurnCurrentOutcome {
        self.evaluate(cancellation_token.is_cancelled(), current_turn)
    }
}

/// 当前 turn 保护的判断结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTurnCurrentOutcome {
    Current,
    Interrupted,
}

/// 取消请求命中的 turn 保护。
///
/// 宿主可能同时维护持久化的 `current_turn` 和内存中的 active turn 控制记录。
/// 取消路径只要命中其中任意一处，就应允许取消继续执行；两处都不命中时，
/// 该请求视为 stale，不应改写宿主状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnCancellationGuard<TurnId> {
    turn_id: TurnId,
}

impl<TurnId> AgentTurnCancellationGuard<TurnId> {
    pub fn new(turn_id: TurnId) -> Self {
        Self { turn_id }
    }
}

impl<TurnId> AgentTurnCancellationGuard<TurnId>
where
    TurnId: PartialEq,
{
    /// 判断目标 turn 是否仍由宿主当前状态或 active turn 控制记录持有。
    pub fn evaluate(
        &self,
        current_turn: Option<&TurnId>,
        active_turn: Option<&TurnId>,
    ) -> AgentTurnCancellationOutcome {
        let matches_current = current_turn.is_some_and(|current| *current == self.turn_id);
        let matches_active = active_turn.is_some_and(|active| *active == self.turn_id);
        if matches_current || matches_active {
            AgentTurnCancellationOutcome::TargetActive
        } else {
            AgentTurnCancellationOutcome::Stale
        }
    }
}

/// 取消请求的通用命中结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTurnCancellationOutcome {
    TargetActive,
    Stale,
}

/// 单轮运行的最终状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnResultStatus {
    Completed,
    Aborted,
    Errored,
}

/// 单轮被中止或出错的结构化原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnAbortReason {
    Interrupted,
    BudgetLimited,
    Shutdown,
    ProviderError,
    ToolError,
}

impl TurnAbortReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interrupted => "interrupted",
            Self::BudgetLimited => "budgetLimited",
            Self::Shutdown => "shutdown",
            Self::ProviderError => "providerError",
            Self::ToolError => "toolError",
        }
    }
}

/// 单轮核心编译结果。
#[derive(Debug, Clone)]
pub struct TurnResult {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub model: String,
    pub usage: TokenUsage,
    pub last_context_tokens: Option<u64>,
    pub context_compactions: Vec<ContextCompactionSnapshot>,
    pub mode: CompileMode,
    pub session_message_count: usize,
    pub status: TurnResultStatus,
    pub abort_reason: Option<TurnAbortReason>,
    pub error: Option<String>,
    pub budget_limit_kind: Option<BudgetLimitKind>,
    pub budget_usage: Option<BudgetUsage>,
    /// Structured trace events recorded during this turn (if tracing was enabled).
    pub trace_events: Vec<TraceEvent>,
}

/// 单轮运行时统计快照。
///
/// 宿主产品使用该结构把 pl-core 的 usage/context/compaction 状态投影到自身
/// store 或 Web API；哪些统计需要暴露由 pl-core 统一判断，产品层不再直接解释
/// `TurnResult` 的底层字段组合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRuntimeSnapshot {
    usage: Option<TokenUsage>,
    last_context_tokens: Option<u64>,
    latest_context_compaction: Option<ContextCompactionSnapshot>,
}

impl TurnRuntimeSnapshot {
    pub fn usage(&self) -> Option<&TokenUsage> {
        self.usage.as_ref()
    }

    pub fn last_context_tokens(&self) -> Option<u64> {
        self.last_context_tokens
    }

    pub fn latest_context_compaction(&self) -> Option<&ContextCompactionSnapshot> {
        self.latest_context_compaction.as_ref()
    }
}

impl TurnResult {
    pub fn runtime_snapshot(&self) -> TurnRuntimeSnapshot {
        TurnRuntimeSnapshot {
            usage: (self.usage.total_tokens > 0).then_some(self.usage.clone()),
            last_context_tokens: self.last_context_tokens,
            latest_context_compaction: self.context_compactions.last().cloned(),
        }
    }
}

/// 宿主可投影到自身状态机的单轮归一化结果。
///
/// `pl-core` 负责解释通用 turn 终态和中止原因；宿主仍负责把该结果映射到
/// 自己的持久化状态、Web 事件和产品错误类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    status: TurnOutcomeStatus,
    final_text: Option<String>,
    error: Option<String>,
    return_error: Option<TurnReturnError>,
}

impl TurnOutcome {
    pub fn status(&self) -> TurnOutcomeStatus {
        self.status
    }

    pub fn final_text(&self) -> Option<&str> {
        self.final_text.as_deref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn return_error(&self) -> Option<&TurnReturnError> {
        self.return_error.as_ref()
    }

    pub fn from_result(result: &TurnResult) -> Self {
        let final_text = (!result.content.trim().is_empty()).then_some(result.content.clone());
        let status = match result.status {
            TurnResultStatus::Completed => TurnOutcomeStatus::Completed,
            TurnResultStatus::Aborted
                if result.abort_reason == Some(TurnAbortReason::Interrupted) =>
            {
                TurnOutcomeStatus::Cancelled
            }
            TurnResultStatus::Aborted | TurnResultStatus::Errored => TurnOutcomeStatus::Failed,
        };
        let return_error = match status {
            TurnOutcomeStatus::Completed => None,
            TurnOutcomeStatus::Cancelled => Some(TurnReturnError::Cancelled),
            TurnOutcomeStatus::Failed => Some(TurnReturnError::Failed(
                result
                    .error
                    .clone()
                    .unwrap_or_else(|| "pl-core turn failed".to_string()),
            )),
        };
        Self {
            status,
            final_text,
            error: result.error.clone(),
            return_error,
        }
    }
}

/// 宿主无关的单轮完成分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcomeStatus {
    Completed,
    Cancelled,
    Failed,
}

/// 宿主在完成持久化和事件投影后应向调用方返回的通用错误分类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnReturnError {
    Cancelled,
    Failed(String),
}

/// 宿主错误在 turn 边界上的通用分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnReturnErrorKind {
    Cancelled,
    Failed,
}

impl TurnReturnError {
    /// 从宿主错误文本和宿主提供的通用分类构造 `pl-core` turn 返回错误。
    pub fn from_host_error(error: impl fmt::Display, kind: TurnReturnErrorKind) -> Self {
        match kind {
            TurnReturnErrorKind::Cancelled => Self::Cancelled,
            TurnReturnErrorKind::Failed => Self::Failed(error.to_string()),
        }
    }
}

/// 宿主可投影到自身状态机的单轮错误终止语义。
///
/// `pl-core` 负责把取消、失败等通用返回错误解释为稳定的 turn 终态、
/// 模型可见/界面可见错误文本和是否应广播错误事件；宿主只需要把该结果
/// 映射到自身协议枚举与持久化格式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnErrorProjection {
    status: TurnOutcomeStatus,
    error_message: Option<String>,
    should_publish_error: bool,
}

impl TurnErrorProjection {
    pub fn status(&self) -> TurnOutcomeStatus {
        self.status
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    pub fn should_publish_error(&self) -> bool {
        self.should_publish_error
    }

    pub fn from_return_error(error: TurnReturnError) -> Self {
        match error {
            TurnReturnError::Cancelled => Self {
                status: TurnOutcomeStatus::Cancelled,
                error_message: None,
                should_publish_error: false,
            },
            TurnReturnError::Failed(message) => Self {
                status: TurnOutcomeStatus::Failed,
                error_message: Some(message),
                should_publish_error: true,
            },
        }
    }
}

/// 检查 turn 取消信号并返回宿主无关的 turn 错误分类。
///
/// 宿主在 turn 编排中的任意 checkpoint 都应复用该函数，把取消语义统一投影
/// 为 `TurnReturnError::Cancelled`，再由产品 adapter 映射到自身错误类型。
pub fn ensure_turn_not_cancelled(
    cancellation_token: &CancellationToken,
) -> Result<(), TurnReturnError> {
    if cancellation_token.is_cancelled() {
        Err(TurnReturnError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn default_budget_policy_matches_codex_style() {
        let policy = BudgetPolicy::default();

        assert_eq!(policy.agent_budget.max_agents, 16);
        assert_eq!(policy.agent_budget.max_depth, 3);
    }

    #[test]
    fn turn_budget_has_generous_wall_clock() {
        let root = TurnBudget::root_default();
        let child = TurnBudget::child_default();

        assert_eq!(root.wall_clock_ms, 1_800_000);
        assert_eq!(child.wall_clock_ms, 1_800_000);
    }

    #[test]
    fn compile_mode_from_label_keeps_old_values_auto_compatible() {
        assert_eq!(CompileMode::from_label("plan"), CompileMode::Plan);
        assert_eq!(CompileMode::from_label("auto"), CompileMode::Auto);
        assert_eq!(CompileMode::from_label("manual"), CompileMode::Auto);
        assert_eq!(CompileMode::from_label(""), CompileMode::Auto);
    }

    #[test]
    fn compile_mode_default_is_auto() {
        assert_eq!(CompileMode::default(), CompileMode::Auto);
    }

    fn turn_result(status: TurnResultStatus, abort_reason: Option<TurnAbortReason>) -> TurnResult {
        TurnResult {
            content: "  final text  ".to_string(),
            reasoning_content: None,
            model: "deepseek-v4".to_string(),
            usage: TokenUsage::default(),
            last_context_tokens: None,
            context_compactions: Vec::new(),
            mode: CompileMode::Auto,
            session_message_count: 0,
            status,
            abort_reason,
            error: None,
            budget_limit_kind: None,
            budget_usage: None,
            trace_events: Vec::new(),
        }
    }

    #[test]
    fn turn_outcome_classifies_completed_cancelled_and_failed_results() {
        assert_eq!(
            TurnOutcome::from_result(&turn_result(TurnResultStatus::Completed, None)),
            TurnOutcome {
                status: TurnOutcomeStatus::Completed,
                final_text: Some("  final text  ".to_string()),
                error: None,
                return_error: None,
            }
        );

        assert_eq!(
            TurnOutcome::from_result(&turn_result(
                TurnResultStatus::Aborted,
                Some(TurnAbortReason::Interrupted),
            )),
            TurnOutcome {
                status: TurnOutcomeStatus::Cancelled,
                final_text: Some("  final text  ".to_string()),
                error: None,
                return_error: Some(TurnReturnError::Cancelled),
            }
        );

        assert_eq!(
            TurnOutcome::from_result(&turn_result(TurnResultStatus::Errored, None)),
            TurnOutcome {
                status: TurnOutcomeStatus::Failed,
                final_text: Some("  final text  ".to_string()),
                error: None,
                return_error: Some(TurnReturnError::Failed("pl-core turn failed".to_string(),)),
            }
        );
    }

    #[test]
    fn turn_error_projection_distinguishes_cancelled_and_failed_returns() {
        assert_eq!(
            TurnErrorProjection::from_return_error(TurnReturnError::Cancelled),
            TurnErrorProjection {
                status: TurnOutcomeStatus::Cancelled,
                error_message: None,
                should_publish_error: false,
            }
        );

        assert_eq!(
            TurnErrorProjection::from_return_error(TurnReturnError::Failed(
                "model stream disconnected".to_string(),
            )),
            TurnErrorProjection {
                status: TurnOutcomeStatus::Failed,
                error_message: Some("model stream disconnected".to_string()),
                should_publish_error: true,
            }
        );
    }

    #[test]
    fn turn_return_error_builds_from_host_error_classification() {
        assert_eq!(
            TurnReturnError::from_host_error("cancelled by user", TurnReturnErrorKind::Cancelled),
            TurnReturnError::Cancelled
        );
        assert_eq!(
            TurnReturnError::from_host_error(
                "model stream disconnected",
                TurnReturnErrorKind::Failed
            ),
            TurnReturnError::Failed("model stream disconnected".to_string())
        );
    }

    #[test]
    fn turn_projection_types_hide_fields_behind_accessors() {
        let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/turn.rs"))
            .expect("turn source");

        let runtime_snapshot_fields = source
            .split("pub struct TurnRuntimeSnapshot {")
            .nth(1)
            .expect("runtime snapshot struct")
            .split("impl TurnRuntimeSnapshot")
            .next()
            .expect("runtime snapshot fields");
        let runtime_snapshot_impl = source
            .split("impl TurnRuntimeSnapshot")
            .nth(1)
            .expect("runtime snapshot impl")
            .split("impl TurnResult")
            .next()
            .expect("runtime snapshot impl body");
        for accessor in [
            "pub fn usage(",
            "pub fn last_context_tokens(",
            "pub fn latest_context_compaction(",
        ] {
            assert!(
                runtime_snapshot_impl.contains(accessor),
                "turn 运行时快照应由 pl-core accessor 暴露 `{accessor}`"
            );
        }
        assert!(
            !runtime_snapshot_fields.contains("pub usage:")
                && !runtime_snapshot_fields.contains("pub last_context_tokens:")
                && !runtime_snapshot_fields.contains("pub latest_context_compaction:"),
            "宿主不应直接读取 TurnRuntimeSnapshot 字段"
        );

        let outcome_fields = source
            .split("pub struct TurnOutcome {")
            .nth(1)
            .expect("turn outcome struct")
            .split("impl TurnOutcome")
            .next()
            .expect("turn outcome fields");
        let outcome_impl = source
            .split("impl TurnOutcome")
            .nth(1)
            .expect("turn outcome impl")
            .split("/// 宿主无关的单轮完成分类。")
            .next()
            .expect("turn outcome impl body");
        for accessor in [
            "pub fn status(",
            "pub fn final_text(",
            "pub fn error(",
            "pub fn return_error(",
        ] {
            assert!(
                outcome_impl.contains(accessor),
                "turn outcome 应由 pl-core accessor 暴露 `{accessor}`"
            );
        }
        assert!(
            !outcome_fields.contains("pub status:")
                && !outcome_fields.contains("pub final_text:")
                && !outcome_fields.contains("pub error:")
                && !outcome_fields.contains("pub return_error:"),
            "宿主不应直接读取 TurnOutcome 字段"
        );

        let error_projection_fields = source
            .split("pub struct TurnErrorProjection {")
            .nth(1)
            .expect("turn error projection struct")
            .split("impl TurnErrorProjection")
            .next()
            .expect("turn error projection fields");
        let error_projection_impl = source
            .split("impl TurnErrorProjection")
            .nth(1)
            .expect("turn error projection impl")
            .split("/// 检查 turn 取消信号")
            .next()
            .expect("turn error projection impl body");
        for accessor in [
            "pub fn status(",
            "pub fn error_message(",
            "pub fn should_publish_error(",
        ] {
            assert!(
                error_projection_impl.contains(accessor),
                "turn 错误投影应由 pl-core accessor 暴露 `{accessor}`"
            );
        }
        assert!(
            !error_projection_fields.contains("pub status:")
                && !error_projection_fields.contains("pub error_message:")
                && !error_projection_fields.contains("pub should_publish_error:"),
            "宿主不应直接读取 TurnErrorProjection 字段"
        );
    }

    #[test]
    fn ensure_turn_not_cancelled_returns_shared_turn_error() {
        let token = CancellationToken::new();

        assert_eq!(ensure_turn_not_cancelled(&token), Ok(()));

        token.cancel();

        assert_eq!(
            ensure_turn_not_cancelled(&token),
            Err(TurnReturnError::Cancelled)
        );
    }

    #[test]
    fn turn_result_runtime_snapshot_exposes_usage_and_context_state() {
        let mut result = turn_result(TurnResultStatus::Completed, None);
        result.usage = TokenUsage {
            prompt_tokens: 11,
            cached_prompt_tokens: 3,
            completion_tokens: 5,
            reasoning_tokens: 2,
            total_tokens: 16,
        };
        result.last_context_tokens = Some(1234);
        result.context_compactions = vec![
            crate::context_compaction::ContextCompactionSnapshot {
                trigger: crate::context_compaction::ContextCompactionTrigger::EstimatedTokens,
                tokens_before: 900,
                estimated_request_tokens: 910,
                provider_prompt_tokens: None,
                auto_compact_limit: 800,
                replacement_tokens: Some(100),
                summary: "older".to_string(),
            },
            crate::context_compaction::ContextCompactionSnapshot {
                trigger: crate::context_compaction::ContextCompactionTrigger::ProviderPromptTokens,
                tokens_before: 1200,
                estimated_request_tokens: 1210,
                provider_prompt_tokens: Some(1234),
                auto_compact_limit: 1000,
                replacement_tokens: Some(120),
                summary: "latest".to_string(),
            },
        ];

        assert_eq!(
            result.runtime_snapshot(),
            TurnRuntimeSnapshot {
                usage: Some(result.usage.clone()),
                last_context_tokens: Some(1234),
                latest_context_compaction: result.context_compactions.last().cloned(),
            }
        );
    }

    #[test]
    fn turn_result_runtime_snapshot_omits_zero_usage() {
        let result = turn_result(TurnResultStatus::Completed, None);

        assert_eq!(
            result.runtime_snapshot(),
            TurnRuntimeSnapshot {
                usage: None,
                last_context_tokens: None,
                latest_context_compaction: None,
            }
        );
    }

    #[test]
    fn permission_mode_from_label_keeps_unknown_values_safe() {
        assert_eq!(
            PermissionMode::from_label("request-approval"),
            PermissionMode::RequestApproval
        );
        assert_eq!(
            PermissionMode::from_label("auto-review"),
            PermissionMode::AutoReview
        );
        assert_eq!(
            PermissionMode::from_label("workspace-write"),
            PermissionMode::RequestApproval
        );
        assert_eq!(
            PermissionMode::from_label("full-access"),
            PermissionMode::FullAccess
        );
        assert_eq!(
            PermissionMode::from_label("old-auto-allow"),
            PermissionMode::RequestApproval
        );
        assert!(PermissionMode::FullAccess.allows_workspace_escape());
        assert!(!PermissionMode::RequestApproval.allows_workspace_escape());
    }

    #[test]
    fn budget_tracker_records_observability() {
        let mut tracker = BudgetTracker::new(TurnBudget::new(60_000));

        tracker.record_model_step();
        tracker.record_tool_call("bash");
        tracker.record_tool_call("wait_agent");

        let usage = tracker.usage();
        assert_eq!(usage.model_steps, 1);
        assert_eq!(usage.tool_calls, 1);
        assert_eq!(usage.wait_calls, 1);
    }

    #[test]
    fn budget_tracker_only_enforces_wall_clock() {
        let mut tracker = BudgetTracker::new(TurnBudget::new(60_000));

        // Model steps 和 tool calls 不再受限制
        for _ in 0..200 {
            tracker.record_model_step();
            tracker.record_tool_call("bash");
        }

        // Wall-clock 未超，不应触发限制
        assert!(tracker.check_wall_clock().is_ok());

        let usage = tracker.usage();
        assert_eq!(usage.model_steps, 200);
        assert_eq!(usage.tool_calls, 200);
    }

    #[tokio::test]
    async fn turn_task_handle_passes_shared_cancellation_token_to_task() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let handle = TurnTaskHandle::spawn_with_token(|token| async move {
            started_tx.send(token.is_cancelled()).unwrap();
            token.cancelled().await;
        });

        assert_eq!(started_rx.await.unwrap(), false);

        handle.cancel();

        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            handle.cancellation_token().cancelled(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn turn_task_handle_aborts_after_grace_when_task_ignores_cancel() {
        struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

        impl Drop for DropSignal {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }

        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let handle = TurnTaskHandle::spawn_with_token(|_token| async move {
            let _drop_signal = DropSignal(Some(dropped_tx));
            std::future::pending::<()>().await;
        });

        handle.cancel_and_abort_after(std::time::Duration::from_millis(1));

        tokio::time::timeout(std::time::Duration::from_millis(200), dropped_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(handle.is_cancelled());
    }

    #[test]
    fn active_turn_control_owns_task_cancellation_boundary() {
        let token = CancellationToken::new();
        let control = ActiveTurnControl::new(
            "turn-a".to_string(),
            "session-a".to_string(),
            TurnTaskHandle::from_external_token(token.clone()),
        );

        assert_eq!(control.turn_id, "turn-a");
        assert_eq!(control.session_id, "session-a");

        control.cancel_task();

        assert!(token.is_cancelled());
    }

    #[test]
    fn active_turn_slot_installs_reads_and_takes_matching_turn() {
        let slot = ActiveTurnSlot::new();
        let control = ActiveTurnControl::new(
            "turn-a".to_string(),
            "session-a".to_string(),
            TurnTaskHandle::from_external_token(CancellationToken::new()),
        );

        assert!(slot.current().is_none());
        slot.set(control);

        assert_eq!(
            slot.current().map(|control| control.session_id),
            Some("session-a".to_string())
        );
        assert!(slot.take_if_turn(&"turn-b".to_string()).is_none());
        assert!(slot.current().is_some());

        let taken = slot.take_if_turn(&"turn-a".to_string()).unwrap();

        assert_eq!(taken.session_id, "session-a");
        assert!(slot.current().is_none());
    }

    #[test]
    fn agent_turn_start_transition_builds_running_mutation() {
        let transition =
            AgentTurnStartTransition::new("turn-a".to_string(), "running".to_string(), 42);

        let outcome = transition.evaluate(true);

        assert_eq!(
            outcome,
            AgentTurnStartOutcome::Started(AgentTurnStartMutation {
                status: "running".to_string(),
                current_turn: Some("turn-a".to_string()),
                updated_at: 42,
                last_error: None,
                cancel_requested: false,
            })
        );
    }

    #[test]
    fn agent_turn_start_transition_reports_busy_without_mutation() {
        let transition =
            AgentTurnStartTransition::new("turn-a".to_string(), "running".to_string(), 42);

        assert_eq!(transition.evaluate(false), AgentTurnStartOutcome::Busy);
    }

    #[test]
    fn agent_turn_completion_transition_builds_terminal_mutation() {
        let transition = AgentTurnCompletionTransition::new(
            "turn-a".to_string(),
            "completed".to_string(),
            42,
            Some("finished".to_string()),
        );
        let current_turn = Some("turn-a".to_string());

        let outcome = transition.evaluate(current_turn.as_ref());

        assert_eq!(
            outcome,
            AgentTurnCompletionOutcome::Completed(AgentTurnCompletionMutation {
                status: "completed".to_string(),
                current_turn: None,
                updated_at: 42,
                last_error: Some("finished".to_string()),
            })
        );
    }

    #[test]
    fn agent_turn_completion_transition_reports_stale_without_mutation() {
        let transition = AgentTurnCompletionTransition::new(
            "turn-a".to_string(),
            "completed".to_string(),
            42,
            None,
        );
        let current_turn = Some("turn-b".to_string());

        assert_eq!(
            transition.evaluate(current_turn.as_ref()),
            AgentTurnCompletionOutcome::Stale
        );
    }

    #[test]
    fn agent_turn_status_transition_builds_status_mutation_when_current() {
        let transition = AgentTurnStatusTransition::new(
            "turn-a".to_string(),
            "waiting".to_string(),
            42,
            AgentTurnStatusGuard::EnforceCurrent,
        );
        let current_turn = Some("turn-a".to_string());

        let outcome = transition.evaluate(false, current_turn.as_ref());

        assert_eq!(
            outcome,
            AgentTurnStatusOutcome::Applied(AgentTurnStatusMutation {
                status: "waiting".to_string(),
                updated_at: 42,
            })
        );
    }

    #[test]
    fn agent_turn_status_transition_cancels_enforced_stale_turn() {
        let transition = AgentTurnStatusTransition::new(
            "turn-a".to_string(),
            "waiting".to_string(),
            42,
            AgentTurnStatusGuard::EnforceCurrent,
        );
        let current_turn = Some("turn-b".to_string());

        assert_eq!(
            transition.evaluate(false, current_turn.as_ref()),
            AgentTurnStatusOutcome::Cancelled
        );
    }

    #[test]
    fn agent_turn_status_transition_cancels_cancelled_request() {
        let transition = AgentTurnStatusTransition::new(
            "turn-a".to_string(),
            "waiting".to_string(),
            42,
            AgentTurnStatusGuard::AllowStale,
        );
        let current_turn = Some("turn-a".to_string());

        assert_eq!(
            transition.evaluate(true, current_turn.as_ref()),
            AgentTurnStatusOutcome::Cancelled
        );
    }

    #[test]
    fn agent_turn_status_transition_evaluates_cancellation_token() {
        let current_turn = Some("turn-a".to_string());
        let token = CancellationToken::new();
        let transition = AgentTurnStatusTransition::new(
            "turn-a".to_string(),
            "waiting".to_string(),
            42,
            AgentTurnStatusGuard::AllowStale,
        );

        assert_eq!(
            transition.evaluate_with_token(&token, current_turn.as_ref()),
            AgentTurnStatusOutcome::Applied(AgentTurnStatusMutation {
                status: "waiting".to_string(),
                updated_at: 42,
            })
        );

        token.cancel();
        let transition = AgentTurnStatusTransition::new(
            "turn-a".to_string(),
            "waiting".to_string(),
            42,
            AgentTurnStatusGuard::AllowStale,
        );

        assert_eq!(
            transition.evaluate_with_token(&token, current_turn.as_ref()),
            AgentTurnStatusOutcome::Cancelled
        );
    }

    #[test]
    fn agent_turn_status_transition_allows_unenforced_stale_turn() {
        let transition = AgentTurnStatusTransition::new(
            "turn-a".to_string(),
            "waiting".to_string(),
            42,
            AgentTurnStatusGuard::AllowStale,
        );
        let current_turn = Some("turn-b".to_string());

        assert!(matches!(
            transition.evaluate(false, current_turn.as_ref()),
            AgentTurnStatusOutcome::Applied(_)
        ));
    }

    #[test]
    fn agent_turn_current_guard_allows_matching_current_turn() {
        let guard = AgentTurnCurrentGuard::new("turn-a".to_string());
        let current_turn = Some("turn-a".to_string());

        assert_eq!(
            guard.evaluate(false, current_turn.as_ref()),
            AgentTurnCurrentOutcome::Current
        );
    }

    #[test]
    fn agent_turn_current_guard_interrupts_cancelled_request() {
        let guard = AgentTurnCurrentGuard::new("turn-a".to_string());
        let current_turn = Some("turn-a".to_string());

        assert_eq!(
            guard.evaluate(true, current_turn.as_ref()),
            AgentTurnCurrentOutcome::Interrupted
        );
    }

    #[test]
    fn agent_turn_current_guard_evaluates_cancellation_token() {
        let guard = AgentTurnCurrentGuard::new("turn-a".to_string());
        let current_turn = Some("turn-a".to_string());
        let token = CancellationToken::new();

        assert_eq!(
            guard.evaluate_with_token(&token, current_turn.as_ref()),
            AgentTurnCurrentOutcome::Current
        );

        token.cancel();

        assert_eq!(
            guard.evaluate_with_token(&token, current_turn.as_ref()),
            AgentTurnCurrentOutcome::Interrupted
        );
    }

    #[test]
    fn agent_turn_current_guard_interrupts_missing_or_stale_turn() {
        let guard = AgentTurnCurrentGuard::new("turn-a".to_string());
        let missing_turn: Option<String> = None;
        let stale_turn = Some("turn-b".to_string());

        assert_eq!(
            guard.evaluate(false, missing_turn.as_ref()),
            AgentTurnCurrentOutcome::Interrupted
        );
        assert_eq!(
            guard.evaluate(false, stale_turn.as_ref()),
            AgentTurnCurrentOutcome::Interrupted
        );
    }

    #[test]
    fn agent_turn_cancellation_guard_allows_current_or_active_turn() {
        let guard = AgentTurnCancellationGuard::new("turn-a".to_string());
        let matching_current = Some("turn-a".to_string());
        let matching_active = Some("turn-a".to_string());
        let stale_current = Some("turn-b".to_string());

        assert_eq!(
            guard.evaluate(matching_current.as_ref(), None),
            AgentTurnCancellationOutcome::TargetActive
        );
        assert_eq!(
            guard.evaluate(stale_current.as_ref(), matching_active.as_ref()),
            AgentTurnCancellationOutcome::TargetActive
        );
    }

    #[test]
    fn agent_turn_cancellation_guard_reports_stale_without_matching_state() {
        let guard = AgentTurnCancellationGuard::new("turn-a".to_string());
        let stale_current = Some("turn-b".to_string());
        let stale_active = Some("turn-c".to_string());
        let missing_turn: Option<String> = None;

        assert_eq!(
            guard.evaluate(stale_current.as_ref(), stale_active.as_ref()),
            AgentTurnCancellationOutcome::Stale
        );
        assert_eq!(
            guard.evaluate(missing_turn.as_ref(), missing_turn.as_ref()),
            AgentTurnCancellationOutcome::Stale
        );
    }
}

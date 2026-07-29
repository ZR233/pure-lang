use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

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
    pub execution_policy: Option<crate::AgentExecutionPolicy>,
    pub(crate) checkpoint: Option<crate::AgentTurnCheckpointHandle>,
    pub(crate) mailbox: Option<crate::agent_runtime::AgentTurnMailboxHandle>,
}

impl TurnOptions {
    pub fn new(tool_approval_policy: ToolApprovalPolicy) -> Self {
        Self {
            tool_approval_policy,
            permission_mode: PermissionMode::RequestApproval,
            interaction_callback: None,
            cancellation_token: None,
            tool_execution_mode: ToolExecutionMode::ModelDefault,
            prompt_cache_key: None,
            user_input_mode: UserInputMode::AwaitResponse,
            execution_policy: None,
            checkpoint: None,
            mailbox: None,
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

    /// 使用宿主编译出的数据化工具与协作策略。
    pub fn with_execution_policy(mut self, policy: crate::AgentExecutionPolicy) -> Self {
        self.execution_policy = Some(policy);
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
            .field("execution_policy", &self.execution_policy)
            .field("checkpoint", &self.checkpoint)
            .field("mailbox", &self.mailbox)
            .finish()
    }
}

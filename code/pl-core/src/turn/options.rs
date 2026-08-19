use std::sync::Arc;
#[cfg(any(test, debug_assertions))]
use std::time::Duration;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub type InteractionFuture = BoxFuture<'static, pl_protocol::InteractionResolution>;
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

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "request-approval" => Some(Self::RequestApproval),
            "auto-review" => Some(Self::AutoReview),
            "full-access" => Some(Self::FullAccess),
            _ => None,
        }
    }

    pub fn allows_workspace_escape(self) -> bool {
        matches!(self, Self::FullAccess)
    }

    pub fn is_default(&self) -> bool {
        matches!(self, Self::RequestApproval)
    }
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
    pub permission_mode: PermissionMode,
    pub interaction_callback: Option<InteractionCallback>,
    pub cancellation_token: Option<CancellationToken>,
    pub tool_execution_mode: ToolExecutionMode,
    pub prompt_cache_key: Option<String>,
    pub prompt_cache_namespace: Option<String>,
    pub prompt_scope: String,
    pub user_input_mode: UserInputMode,
    pub execution_policy: Option<crate::AgentExecutionPolicy>,
    pub(crate) checkpoint: Option<crate::AgentTurnCheckpointHandle>,
    pub(crate) mailbox: Option<crate::agent_runtime::AgentTurnMailboxHandle>,
    pub(crate) budget_refresh: Option<crate::agent_runtime::TurnBudgetRefreshReceiver>,
    #[cfg(any(test, debug_assertions))]
    debug_context_compaction_timeout: Option<Duration>,
}

impl TurnOptions {
    pub(crate) fn apply_budget_refresh(&self, tracker: &mut super::budget::BudgetTracker) {
        if let Some(accepted_at) = self
            .budget_refresh
            .as_ref()
            .and_then(crate::agent_runtime::TurnBudgetRefreshReceiver::take_latest)
        {
            tracker.refresh_at(accepted_at);
        }
    }

    pub(crate) fn context_compaction_control(
        &self,
    ) -> crate::context_compaction::ContextCompactionControl {
        let control = crate::context_compaction::ContextCompactionControl::default()
            .with_optional_cancellation(self.cancellation_token.clone());
        #[cfg(any(test, debug_assertions))]
        let control = self
            .debug_context_compaction_timeout
            .map_or(control.clone(), |timeout| control.with_timeout(timeout));
        control
    }

    /// 覆盖 debug/test 构建中的 context compaction 硬超时。
    ///
    /// 仅供 scripted runtime 验收夹具缩短等待；release 构建始终使用固定的生产超时。
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn with_debug_context_compaction_timeout(mut self, timeout: Duration) -> Self {
        self.debug_context_compaction_timeout = Some(timeout);
        self
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

    pub fn with_prompt_cache_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.prompt_cache_namespace = Some(namespace.into());
        self
    }

    pub fn with_prompt_scope(mut self, prompt_scope: impl Into<String>) -> Self {
        self.prompt_scope = prompt_scope.into();
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
    }
}

impl Default for TurnOptions {
    fn default() -> Self {
        Self {
            permission_mode: PermissionMode::RequestApproval,
            interaction_callback: None,
            cancellation_token: None,
            tool_execution_mode: ToolExecutionMode::ModelDefault,
            prompt_cache_key: None,
            prompt_cache_namespace: None,
            prompt_scope: "default".to_string(),
            user_input_mode: UserInputMode::AwaitResponse,
            execution_policy: None,
            checkpoint: None,
            mailbox: None,
            budget_refresh: None,
            #[cfg(any(test, debug_assertions))]
            debug_context_compaction_timeout: None,
        }
    }
}

impl std::fmt::Debug for TurnOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnOptions")
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
            .field("prompt_cache_namespace", &self.prompt_cache_namespace)
            .field("prompt_scope", &self.prompt_scope)
            .field("user_input_mode", &self.user_input_mode)
            .field("execution_policy", &self.execution_policy)
            .field("checkpoint", &self.checkpoint)
            .field("mailbox", &self.mailbox)
            .field("budget_refresh", &self.budget_refresh)
            .finish()
    }
}

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pl_model::TokenUsage;
use pl_protocol::TraceEvent;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// 工具分发循环默认最大迭代次数。
pub const DEFAULT_MAX_TOOL_ITERATIONS: usize = 10;

/// 编译请求的执行模式。
///
/// `Plan` 产出规划与解释，也可以在已注册工具边界内做只读探索；
/// `Auto` 允许模型生成更主动的编译步骤和子任务。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    #[default]
    Plan,
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
}

/// 单轮核心编译请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRequest {
    pub prompt: String,
    pub mode: CompileMode,
    pub workspace_instructions: Option<String>,
    /// 工具分发循环最大迭代次数（默认 10）。
    pub max_tool_iterations: usize,
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>, mode: CompileMode) -> Self {
        Self {
            prompt: prompt.into(),
            mode,
            workspace_instructions: None,
            max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
        }
    }

    pub fn with_workspace_instructions(mut self, instructions: String) -> Self {
        self.workspace_instructions = Some(instructions);
        self
    }

    pub fn with_max_tool_iterations(mut self, max: usize) -> Self {
        self.max_tool_iterations = max;
        self
    }
}

pub type ToolApprovalFuture = Pin<Box<dyn Future<Output = ToolApprovalDecision> + Send>>;
pub type ToolApprovalCallback =
    Arc<dyn Fn(ToolApprovalRequest) -> ToolApprovalFuture + Send + Sync>;

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
    pub parent_subagent_id: Option<String>,
}

/// 单次工具调用审批结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolApprovalDecision {
    Approved,
    Denied { reason: String },
}

/// 单轮运行选项。
///
/// 用于前端控制工具审批等运行时行为。默认值保持历史行为：已注册工具自动执行。
#[derive(Clone)]
pub struct TurnOptions {
    pub tool_approval_policy: ToolApprovalPolicy,
    pub tool_approval_callback: Option<ToolApprovalCallback>,
    pub cancellation_token: Option<CancellationToken>,
}

impl TurnOptions {
    pub fn new(tool_approval_policy: ToolApprovalPolicy) -> Self {
        Self {
            tool_approval_policy,
            tool_approval_callback: None,
            cancellation_token: None,
        }
    }

    pub fn manual(callback: ToolApprovalCallback) -> Self {
        Self {
            tool_approval_policy: ToolApprovalPolicy::Manual,
            tool_approval_callback: Some(callback),
            cancellation_token: None,
        }
    }

    pub fn deny_all() -> Self {
        Self::new(ToolApprovalPolicy::DenyAll)
    }

    pub fn with_cancellation(mut self, cancellation_token: CancellationToken) -> Self {
        self.cancellation_token = Some(cancellation_token);
        self
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
            .field(
                "tool_approval_callback",
                &self.tool_approval_callback.as_ref().map(|_| "<callback>"),
            )
            .field(
                "cancellation_token",
                &self.cancellation_token.as_ref().map(|_| "<token>"),
            )
            .finish()
    }
}

/// 单轮运行的最终状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnResultStatus {
    Completed,
    Interrupted,
}

/// 单轮核心编译结果。
#[derive(Debug, Clone)]
pub struct TurnResult {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub model: String,
    pub usage: TokenUsage,
    pub mode: CompileMode,
    pub session_message_count: usize,
    pub status: TurnResultStatus,
    /// Structured trace events recorded during this turn (if tracing was enabled).
    pub trace_events: Vec<TraceEvent>,
}

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pl_model::TokenUsage;
use pl_protocol::{BudgetLimitKind, BudgetUsage, TraceEvent};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// 旧接口的工具分发循环默认值（保留向后兼容）。
pub const DEFAULT_MAX_TOOL_ITERATIONS: usize = 10;

/// Agent 树结构限制常量。
pub const AGENT_MAX_COUNT: usize = 16;
pub const AGENT_MAX_DEPTH: u32 = 3;
/// 默认 wall-clock 安全上限（10 分钟），参考 Codex 的 agent_job_max_runtime_seconds。
pub const DEFAULT_WALL_CLOCK_MS: u64 = 600_000;

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

    pub fn from_legacy_max_tool_iterations(_max: usize) -> Self {
        Self::root_default()
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
    pub prompt: String,
    pub mode: CompileMode,
    pub workspace_instructions: Option<String>,
    pub budget: TurnBudget,
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>, mode: CompileMode) -> Self {
        Self {
            prompt: prompt.into(),
            mode,
            workspace_instructions: None,
            budget: TurnBudget::root_default(),
        }
    }

    pub fn with_workspace_instructions(mut self, instructions: String) -> Self {
        self.workspace_instructions = Some(instructions);
        self
    }

    pub fn with_max_tool_iterations(mut self, max: usize) -> Self {
        self.budget = TurnBudget::from_legacy_max_tool_iterations(max);
        self
    }

    pub fn with_budget(mut self, budget: TurnBudget) -> Self {
        self.budget = budget;
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

/// 单轮运行选项。
///
/// 用于前端控制工具审批等运行时行为。默认值保持历史行为：已注册工具自动执行。
#[derive(Clone)]
pub struct TurnOptions {
    pub tool_approval_policy: ToolApprovalPolicy,
    pub tool_approval_callback: Option<ToolApprovalCallback>,
    pub cancellation_token: Option<CancellationToken>,
    pub tool_execution_mode: ToolExecutionMode,
}

impl TurnOptions {
    pub fn new(tool_approval_policy: ToolApprovalPolicy) -> Self {
        Self {
            tool_approval_policy,
            tool_approval_callback: None,
            cancellation_token: None,
            tool_execution_mode: ToolExecutionMode::ModelDefault,
        }
    }

    pub fn manual(callback: ToolApprovalCallback) -> Self {
        Self {
            tool_approval_policy: ToolApprovalPolicy::Manual,
            tool_approval_callback: Some(callback),
            cancellation_token: None,
            tool_execution_mode: ToolExecutionMode::ModelDefault,
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
            .field("tool_execution_mode", &self.tool_execution_mode)
            .finish()
    }
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
    pub mode: CompileMode,
    pub session_message_count: usize,
    pub status: TurnResultStatus,
    pub abort_reason: Option<TurnAbortReason>,
    pub error: Option<String>,
    pub budget_limit_kind: Option<BudgetLimitKind>,
    pub budget_usage: Option<BudgetUsage>,
    /// Structured timeline events recorded during this turn (if tracing was enabled).
    pub timeline_events: Vec<TraceEvent>,
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

        assert_eq!(root.wall_clock_ms, 600_000);
        assert_eq!(child.wall_clock_ms, 600_000);
    }

    #[test]
    fn compile_mode_from_label_keeps_old_values_auto_compatible() {
        assert_eq!(CompileMode::from_label("plan"), CompileMode::Plan);
        assert_eq!(CompileMode::from_label("auto"), CompileMode::Auto);
        assert_eq!(CompileMode::from_label("manual"), CompileMode::Auto);
        assert_eq!(CompileMode::from_label(""), CompileMode::Auto);
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
}

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pl_model::TokenUsage;
use pl_protocol::{BudgetLimitKind, BudgetUsage, TraceEvent};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// 旧接口的工具分发循环默认值。
pub const DEFAULT_MAX_TOOL_ITERATIONS: usize = 10;

pub const ROOT_MODEL_STEP_BUDGET: u32 = 32;
pub const ROOT_TOOL_CALL_BUDGET: u32 = 120;
pub const ROOT_WAIT_BUDGET: u32 = 16;
pub const ROOT_WALL_CLOCK_BUDGET_MS: u64 = 180_000;
pub const CHILD_MODEL_STEP_BUDGET: u32 = 24;
pub const CHILD_TOOL_CALL_BUDGET: u32 = 80;
pub const CHILD_WAIT_BUDGET: u32 = 12;
pub const CHILD_WALL_CLOCK_BUDGET_MS: u64 = 120_000;

/// 单轮模型与工具执行预算。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnBudget {
    pub model_step_budget: u32,
    pub tool_call_budget: u32,
    pub wait_budget: u32,
    pub wall_clock_ms: u64,
}

impl TurnBudget {
    pub fn root_default() -> Self {
        Self {
            model_step_budget: ROOT_MODEL_STEP_BUDGET,
            tool_call_budget: ROOT_TOOL_CALL_BUDGET,
            wait_budget: ROOT_WAIT_BUDGET,
            wall_clock_ms: ROOT_WALL_CLOCK_BUDGET_MS,
        }
    }

    pub fn child_default() -> Self {
        Self {
            model_step_budget: CHILD_MODEL_STEP_BUDGET,
            tool_call_budget: CHILD_TOOL_CALL_BUDGET,
            wait_budget: CHILD_WAIT_BUDGET,
            wall_clock_ms: CHILD_WALL_CLOCK_BUDGET_MS,
        }
    }

    pub fn from_legacy_max_tool_iterations(max: usize) -> Self {
        let max = max.max(1) as u32;
        Self {
            model_step_budget: max,
            tool_call_budget: max,
            wait_budget: ROOT_WAIT_BUDGET,
            wall_clock_ms: ROOT_WALL_CLOCK_BUDGET_MS,
        }
    }
}

/// Agent tree 预算策略。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentBudget {
    pub max_agents: usize,
    pub max_depth: u32,
    pub child_turn_budget: TurnBudget,
}

impl Default for AgentBudget {
    fn default() -> Self {
        Self {
            max_agents: 16,
            max_depth: 3,
            child_turn_budget: TurnBudget::child_default(),
        }
    }
}

/// Turn 与 agent 协作的完整预算策略。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetPolicy {
    pub root_turn_budget: TurnBudget,
    pub agent_budget: AgentBudget,
}

impl Default for BudgetPolicy {
    fn default() -> Self {
        Self {
            root_turn_budget: TurnBudget::root_default(),
            agent_budget: AgentBudget::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BudgetLimit {
    pub kind: BudgetLimitKind,
    pub usage: BudgetUsage,
}

#[derive(Debug, Clone)]
pub(crate) struct BudgetTracker {
    budget: TurnBudget,
    usage: BudgetUsage,
    started_at: std::time::Instant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn default_budget_policy_matches_studio_contract() {
        let policy = BudgetPolicy::default();

        assert_eq!(policy.root_turn_budget.model_step_budget, 32);
        assert_eq!(policy.root_turn_budget.tool_call_budget, 120);
        assert_eq!(policy.root_turn_budget.wait_budget, 16);
        assert_eq!(policy.root_turn_budget.wall_clock_ms, 180_000);
        assert_eq!(policy.agent_budget.max_agents, 16);
        assert_eq!(policy.agent_budget.max_depth, 3);
        assert_eq!(policy.agent_budget.child_turn_budget.model_step_budget, 24);
        assert_eq!(policy.agent_budget.child_turn_budget.tool_call_budget, 80);
        assert_eq!(policy.agent_budget.child_turn_budget.wait_budget, 12);
        assert_eq!(policy.agent_budget.child_turn_budget.wall_clock_ms, 120_000);
    }

    #[test]
    fn budget_tracker_separates_tool_and_wait_calls() {
        let mut tracker = BudgetTracker::new(TurnBudget {
            model_step_budget: 2,
            tool_call_budget: 1,
            wait_budget: 1,
            wall_clock_ms: 60_000,
        });

        tracker.consume_model_step().unwrap();
        tracker.consume_tool_call("bash").unwrap();
        tracker.consume_tool_call("wait_agent").unwrap();

        let usage = tracker.usage();
        assert_eq!(usage.model_steps, 1);
        assert_eq!(usage.tool_calls, 1);
        assert_eq!(usage.wait_calls, 1);
    }

    #[test]
    fn budget_tracker_reports_limit_kind() {
        let mut tracker = BudgetTracker::new(TurnBudget {
            model_step_budget: 1,
            tool_call_budget: 1,
            wait_budget: 1,
            wall_clock_ms: 60_000,
        });

        tracker.consume_model_step().unwrap();
        let limit = tracker.consume_model_step().unwrap_err();

        assert_eq!(limit.kind, BudgetLimitKind::ModelStep);
        assert_eq!(limit.usage.model_steps, 2);
    }
}

impl BudgetTracker {
    pub fn new(budget: TurnBudget) -> Self {
        Self {
            budget,
            usage: BudgetUsage::default(),
            started_at: std::time::Instant::now(),
        }
    }

    pub fn usage(&self) -> BudgetUsage {
        let mut usage = self.usage;
        usage.elapsed_ms = self.started_at.elapsed().as_millis() as u64;
        usage
    }

    pub fn consume_model_step(&mut self) -> std::result::Result<BudgetUsage, BudgetLimit> {
        self.usage.model_steps += 1;
        self.ensure_within_budget(BudgetLimitKind::ModelStep)?;
        Ok(self.usage())
    }

    pub fn consume_tool_call(&mut self, tool_name: &str) -> std::result::Result<(), BudgetLimit> {
        if tool_name == "wait_agent" {
            self.usage.wait_calls += 1;
            self.ensure_within_budget(BudgetLimitKind::Wait)
        } else {
            self.usage.tool_calls += 1;
            self.ensure_within_budget(BudgetLimitKind::ToolCall)
        }
    }

    pub fn check_wall_clock(&self) -> std::result::Result<(), BudgetLimit> {
        self.ensure_within_budget(BudgetLimitKind::WallClock)
    }

    fn ensure_within_budget(
        &self,
        default_kind: BudgetLimitKind,
    ) -> std::result::Result<(), BudgetLimit> {
        let usage = self.usage();
        let kind = if usage.elapsed_ms > self.budget.wall_clock_ms {
            BudgetLimitKind::WallClock
        } else if usage.model_steps > self.budget.model_step_budget {
            BudgetLimitKind::ModelStep
        } else if usage.tool_calls > self.budget.tool_call_budget {
            BudgetLimitKind::ToolCall
        } else if usage.wait_calls > self.budget.wait_budget {
            BudgetLimitKind::Wait
        } else {
            return Ok(());
        };
        Err(BudgetLimit {
            kind: if matches!(kind, BudgetLimitKind::WallClock) {
                kind
            } else {
                default_kind
            },
            usage,
        })
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
    Failed,
    Interrupted,
    BudgetLimited,
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

use pl_model::TokenUsage;
use pl_protocol::{BudgetLimitKind, BudgetUsage};
use pl_trace::TraceEvent;

use crate::context_compaction::ContextCompactionSnapshot;

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
    pub session_message_count: usize,
    pub status: TurnResultStatus,
    pub abort_reason: Option<TurnAbortReason>,
    pub error: Option<String>,
    pub budget_limit_kind: Option<BudgetLimitKind>,
    pub budget_usage: Option<BudgetUsage>,
    /// Structured trace events recorded during this turn (if tracing was enabled).
    pub trace_events: Vec<TraceEvent>,
}

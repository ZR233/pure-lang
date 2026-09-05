use pl_protocol::InferenceTokenUsage;
use pl_protocol::TurnOutcome;
use pl_trace::TraceEvent;

use crate::context_compaction::ContextCompactionSnapshot;

/// 单轮核心编译结果。
#[derive(Debug, Clone)]
pub struct TurnResult {
    /// Immutable per-inference accounting, including compaction and interrupted requests.
    pub billing: pl_protocol::TurnBillingRecord,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub model: String,
    pub usage: InferenceTokenUsage,
    pub last_context_tokens: Option<u64>,
    pub context_compactions: Vec<ContextCompactionSnapshot>,
    pub session_message_count: usize,
    pub outcome: TurnOutcome,
    /// Structured trace events recorded during this turn (if tracing was enabled).
    pub trace_events: Vec<TraceEvent>,
}

impl TurnResult {
    pub fn is_completed(&self) -> bool {
        self.outcome.is_completed()
    }
}

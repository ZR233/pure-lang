use pl_protocol::{
    AgentStatus, BudgetLimitKind, BudgetUsage, Message, RuntimeUsageSnapshot, TraceEvent,
};

use crate::TurnResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub updated_at: i64,
    pub instruction_snapshot: Option<crate::InstructionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolApprovalRecord {
    pub session_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub working_directory: Option<String>,
    pub decision: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSnapshotRecord {
    pub id: String,
    pub session_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: AgentStatus,
    pub summary: Option<String>,
    pub depth: i32,
    pub error: Option<String>,
    pub reason: Option<String>,
    pub budget_limit_kind: Option<BudgetLimitKind>,
    pub budget_usage: Option<BudgetUsage>,
    pub runtime_usage: Option<RuntimeUsageSnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTimelineEventRecord {
    pub event_id: String,
    pub session_id: String,
    pub sequence: i64,
    pub kind: String,
    pub agent_id: Option<String>,
    pub path: Option<String>,
    pub parent_path: Option<String>,
    pub payload_json: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEventRecord {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub created_at: i64,
    pub kind: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRuntimeRecord {
    pub session_id: String,
    pub model: String,
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    pub currency: Option<String>,
    pub estimated_cost: Option<f64>,
    pub estimated_costs: Vec<pl_protocol::RuntimeCostAmount>,
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct StudioPromptOutcome {
    pub result: TurnResult,
    pub messages: Vec<Message>,
    pub timeline_events: Vec<TraceEvent>,
}

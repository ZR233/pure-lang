use crate::{
    AgentStatus, BudgetLimitKind, BudgetUsage, Message, RuntimeUsageSnapshot, StudioMessage,
    StudioPart, StudioTurnStatus,
};

pub use crate::attachment::MaterializedAttachment;
use pl_trace::TraceEvent;

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
    pub visibility: SessionVisibility,
    pub parent_session_id: Option<String>,
    pub instruction_snapshot: Option<crate::InstructionSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionVisibility {
    Active,
    HandoffOrigin,
    Archived,
}

impl SessionVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::HandoffOrigin => "handoffOrigin",
            Self::Archived => "archived",
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRecord {
    pub id: String,
    pub session_id: String,
    pub message_id: Option<String>,
    pub media_type: String,
    pub filename: Option<String>,
    pub storage_path: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioEventRecord {
    pub id: String,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub sequence: i64,
    pub created_at: i64,
    pub kind: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioTurnRecord {
    pub id: String,
    pub session_id: String,
    pub status: StudioTurnStatus,
    pub reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioMessageRecord {
    pub message: StudioMessage,
    pub sequence: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StudioPartRecord {
    pub part: StudioPart,
    pub sequence: i64,
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
    pub estimated_costs: Vec<crate::RuntimeCostAmount>,
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSkillRecord {
    pub session_id: String,
    pub skill_name: String,
    pub source: String,
    pub path: String,
    pub first_turn_id: String,
    pub last_turn_id: String,
    pub last_tool_call_id: String,
    pub activated_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct StudioPromptOutcome {
    pub result: TurnResult,
    pub messages: Vec<Message>,
    pub trace_events: Vec<TraceEvent>,
}

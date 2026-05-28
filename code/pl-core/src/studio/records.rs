use pl_protocol::{AgentStatus, Message, TraceEvent};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEventRecord {
    pub event_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: AgentStatus,
    pub summary: Option<String>,
    pub depth: i32,
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraceEventRecord {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub timestamp: i64,
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
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct StudioPromptOutcome {
    pub result: TurnResult,
    pub messages: Vec<Message>,
    pub trace_events: Vec<TraceEvent>,
}

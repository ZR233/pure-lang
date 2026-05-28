use pl_protocol::AgentStatus;
use serde::{Deserialize, Serialize};

/// Snapshot of an agent known to the current collaboration tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRecord {
    pub id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: AgentStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub depth: u32,
    pub updated_at: i64,
}

/// Append-only agent event persisted for Studio timelines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub error: Option<String>,
    pub depth: u32,
    pub created_at: i64,
}

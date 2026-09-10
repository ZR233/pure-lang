//! Runtime health projections shared by the Studio bridge.
//!
//! Workflow state is intentionally not duplicated here: it is projected from the
//! canonical `AgentWorkingState` into `pl_protocol::ThreadRuntimeSnapshot`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentDirectoryEntry {
    pub id: String,
    pub thread_id: String,
    pub root_thread_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub summary: Option<String>,
    pub depth: u32,
    pub state: pl_protocol::AgentState,
    pub progress: Option<pl_protocol::AgentProgressCheckpoint>,
    pub updated_at: i64,
    pub summary_age_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpHealth {
    pub mcp_servers: Vec<StudioMcpServer>,
    pub active_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspHealth {
    pub lsp_servers: Vec<StudioLspServer>,
    pub active_lsp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpServer {
    pub id: String,
    pub transport: String,
    pub endpoint: String,
    pub source_kind: String,
    pub mutation_policy: String,
    pub state: crate::StudioMcpServerState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspServer {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub language_ids: Vec<String>,
    pub state: crate::StudioLspServerState,
}

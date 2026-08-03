use serde::{Deserialize, Serialize};

use super::{
    StudioAgentDirectoryEntry, StudioLspHealth, StudioMcpHealth, StudioSessionSummary,
    StudioTaskRuntime,
};

/// Studio 产品级事件信封。
///
/// 会话、turn、消息、工具与 interaction 事件统一由 `pl-protocol` 的
/// `SessionEventEnvelope` 表达；这里仅保留不属于单一会话的低频产品状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioProductEventEnvelope {
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: StudioProductEventKind,
}

/// Studio 全局产品事件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum StudioProductEventKind {
    SessionListChanged {
        project_id: String,
        sessions: Vec<StudioSessionSummary>,
    },
    McpHealthChanged {
        health: StudioMcpHealth,
    },
    LspHealthChanged {
        health: StudioLspHealth,
    },
    SessionTaskChanged {
        session_id: String,
        task: Option<Box<StudioTaskRuntime>>,
    },
    AgentDirectoryChanged {
        root_session_id: String,
        agent: StudioAgentDirectoryEntry,
    },
}

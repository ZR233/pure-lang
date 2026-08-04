use pl_protocol::Thread;
use serde::{Deserialize, Serialize};

use super::{StudioAgentDirectoryEntry, StudioLspHealth, StudioMcpHealth, StudioTaskRuntime};

/// Studio 产品级事件信封。
///
/// Thread、Turn、Item 与 interaction 事件统一由 Thread subscription 表达；这里仅
/// 保留不属于单一 Thread 的低频产品状态。
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
    ThreadDirectoryChanged {
        project_id: String,
        threads: Vec<Thread>,
    },
    McpHealthChanged {
        health: StudioMcpHealth,
    },
    LspHealthChanged {
        health: StudioLspHealth,
    },
    TaskChanged {
        root_thread_id: String,
        task: Option<Box<StudioTaskRuntime>>,
    },
    AgentDirectoryChanged {
        root_thread_id: String,
        agent: Box<StudioAgentDirectoryEntry>,
    },
}

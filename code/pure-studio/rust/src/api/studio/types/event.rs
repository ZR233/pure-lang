use super::response::SessionDto;
use super::runtime::{
    BridgeAgentDirectoryEntryDto, BridgeLspHealthDto, BridgeMcpHealthDto, BridgeTaskRuntimeDto,
};
use serde::{Deserialize, Serialize};

/// Flutter Bridge 的 Studio 产品事件信封。
///
/// session 事件通过 `BridgeSessionStreamFrame` 透明传输，不得加入此类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeProductEventEnvelope {
    pub event_id: String,
    pub project_id: Option<String>,
    pub sequence: u64,
    pub created_at: i64,
    pub payload: BridgeProductEventPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeProductEventPayload {
    SessionListChanged {
        project_id: String,
        sessions: Vec<SessionDto>,
    },
    McpHealthChanged {
        health: BridgeMcpHealthDto,
    },
    LspHealthChanged {
        health: BridgeLspHealthDto,
    },
    SessionTaskChanged {
        session_id: String,
        task: Option<Box<BridgeTaskRuntimeDto>>,
    },
    AgentDirectoryChanged {
        root_session_id: String,
        agent: BridgeAgentDirectoryEntryDto,
    },
    Stale {
        lagged_events: u64,
    },
}

impl BridgeProductEventEnvelope {
    pub fn stale(lagged_events: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            event_id: format!("bridge-product-stale-{:x}", now.as_nanos()),
            project_id: None,
            sequence: 0,
            created_at: now.as_secs() as i64,
            payload: BridgeProductEventPayload::Stale { lagged_events },
        }
    }
}

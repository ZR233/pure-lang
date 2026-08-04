use super::runtime::{
    BridgeAgentDirectoryEntryDto, BridgeLspHealthDto, BridgeMcpHealthDto, BridgeTaskRuntimeDto,
};
use super::thread_stream::BridgeThread;
use serde::{Deserialize, Serialize};

/// Flutter Bridge 的 Studio 产品事件信封。
///
/// Thread 高频事件通过 Thread subscription 传输，不得加入此类型。
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
    ThreadDirectoryChanged {
        project_id: String,
        threads: Vec<BridgeThread>,
    },
    McpHealthChanged {
        health: BridgeMcpHealthDto,
    },
    LspHealthChanged {
        health: BridgeLspHealthDto,
    },
    TaskChanged {
        root_thread_id: String,
        task: Option<Box<BridgeTaskRuntimeDto>>,
    },
    AgentDirectoryChanged {
        root_thread_id: String,
        agent: Box<BridgeAgentDirectoryEntryDto>,
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

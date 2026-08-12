use super::runtime::{bridge_agent_directory_entry, bridge_mcp_health, bridge_task_runtime};
use super::thread_stream::bridge_thread;
use crate::api::studio::types::{BridgeProductEventEnvelope, BridgeProductEventPayload};
use pl_studio_runtime::{StudioProductEventEnvelope, StudioProductEventKind};

pub(crate) fn bridge_product_event(
    event: StudioProductEventEnvelope,
) -> BridgeProductEventEnvelope {
    BridgeProductEventEnvelope {
        event_id: event.event_id,
        project_id: event.project_id,
        sequence: event.sequence,
        created_at: event.created_at,
        payload: match event.kind {
            StudioProductEventKind::ThreadDirectoryChanged {
                project_id,
                threads,
            } => BridgeProductEventPayload::ThreadDirectoryChanged {
                project_id,
                threads: threads.into_iter().map(bridge_thread).collect(),
            },
            StudioProductEventKind::McpHealthChanged { health } => {
                BridgeProductEventPayload::McpHealthChanged {
                    health: bridge_mcp_health(health),
                }
            }
            StudioProductEventKind::LspHealthChanged { health } => {
                BridgeProductEventPayload::LspHealthChanged {
                    health: health.into(),
                }
            }
            StudioProductEventKind::TaskChanged {
                root_thread_id,
                task,
            } => BridgeProductEventPayload::TaskChanged {
                root_thread_id,
                task: task.map(|task| Box::new(bridge_task_runtime(*task))),
            },
            StudioProductEventKind::AgentDirectoryChanged {
                root_thread_id,
                agent,
            } => BridgeProductEventPayload::AgentDirectoryChanged {
                root_thread_id,
                agent: Box::new(bridge_agent_directory_entry(*agent)),
            },
        },
    }
}

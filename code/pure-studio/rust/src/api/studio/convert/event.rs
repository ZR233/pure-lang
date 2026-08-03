use super::records::session_summary_dto;
use super::runtime::{
    bridge_agent_directory_entry, bridge_lsp_health, bridge_mcp_health, bridge_task_runtime,
};
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
            StudioProductEventKind::SessionListChanged {
                project_id,
                sessions,
            } => BridgeProductEventPayload::SessionListChanged {
                project_id,
                sessions: sessions.into_iter().map(session_summary_dto).collect(),
            },
            StudioProductEventKind::McpHealthChanged { health } => {
                BridgeProductEventPayload::McpHealthChanged {
                    health: bridge_mcp_health(health),
                }
            }
            StudioProductEventKind::LspHealthChanged { health } => {
                BridgeProductEventPayload::LspHealthChanged {
                    health: bridge_lsp_health(health),
                }
            }
            StudioProductEventKind::SessionTaskChanged { session_id, task } => {
                BridgeProductEventPayload::SessionTaskChanged {
                    session_id,
                    task: task.map(|task| Box::new(bridge_task_runtime(*task))),
                }
            }
            StudioProductEventKind::AgentDirectoryChanged {
                root_session_id,
                agent,
            } => BridgeProductEventPayload::AgentDirectoryChanged {
                root_session_id,
                agent: bridge_agent_directory_entry(agent),
            },
        },
    }
}

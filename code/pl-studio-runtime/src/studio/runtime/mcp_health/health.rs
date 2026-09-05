//! effective MCP 配置与可用性快照到公开健康 DTO 的纯映射。

use std::collections::BTreeMap;

use pl_protocol::StateError;

use crate::config::{EffectiveMcpServerConfig, McpServerStatusKind};
use crate::mcp::{McpAvailabilityKind, McpAvailabilitySnapshot};
use crate::studio::ids::unix_seconds;
use crate::{
    McpAvailable, McpChecking, McpDisabled, McpMissingCredential, McpUnavailable, StudioMcpHealth,
    StudioMcpServer, StudioMcpServerState,
};

pub(super) fn mcp_health_from_effective(
    servers: BTreeMap<String, EffectiveMcpServerConfig>,
    snapshots: BTreeMap<String, McpAvailabilitySnapshot>,
    active_mcp_servers: Vec<String>,
) -> StudioMcpHealth {
    StudioMcpHealth {
        mcp_servers: servers
            .into_iter()
            .map(|(server_id, server)| {
                let snapshot = snapshots.get(&server_id);
                studio_mcp_server(server, snapshot)
            })
            .collect(),
        active_mcp_servers,
    }
}

fn studio_mcp_server(
    server: EffectiveMcpServerConfig,
    snapshot: Option<&McpAvailabilitySnapshot>,
) -> StudioMcpServer {
    let endpoint = server.config.endpoint_summary();
    let state = mcp_server_state(
        server.status_kind,
        server.status_message.as_deref(),
        snapshot,
    );
    StudioMcpServer {
        id: server.id,
        transport: server.config.transport.as_str().to_string(),
        endpoint,
        source_kind: server.source_kind.as_str().to_string(),
        mutation_policy: server.mutation_policy.as_str().to_string(),
        state,
    }
}

fn mcp_server_state(
    status: McpServerStatusKind,
    status_message: Option<&str>,
    snapshot: Option<&McpAvailabilitySnapshot>,
) -> StudioMcpServerState {
    match status {
        McpServerStatusKind::Disabled => StudioMcpServerState::Disabled(McpDisabled::new(
            status_message.unwrap_or("MCP server is disabled in configuration"),
        )),
        McpServerStatusKind::MissingCredential => {
            StudioMcpServerState::MissingCredential(McpMissingCredential::new(
                status_message.unwrap_or("MCP server credential is not configured"),
            ))
        }
        McpServerStatusKind::Enabled => match snapshot {
            Some(snapshot) => match snapshot.availability_kind {
                McpAvailabilityKind::Checking => StudioMcpServerState::Checking(McpChecking::new(
                    snapshot
                        .availability_message
                        .as_deref()
                        .unwrap_or("MCP health check is running"),
                )),
                McpAvailabilityKind::Available => {
                    StudioMcpServerState::Available(McpAvailable::new(
                        snapshot.last_checked_at.unwrap_or_else(unix_seconds),
                        snapshot.tool_count.unwrap_or_default() as u64,
                    ))
                }
                McpAvailabilityKind::Unavailable => {
                    StudioMcpServerState::Unavailable(McpUnavailable::new(
                        snapshot.last_checked_at.unwrap_or_else(unix_seconds),
                        StateError {
                            code: "mcpServerUnavailable".to_string(),
                            message: snapshot
                                .availability_message
                                .clone()
                                .unwrap_or_else(|| "MCP server is unavailable".to_string()),
                            retryable: true,
                        },
                    ))
                }
                McpAvailabilityKind::Disabled => StudioMcpServerState::Disabled(McpDisabled::new(
                    snapshot
                        .availability_message
                        .as_deref()
                        .unwrap_or("MCP server is disabled in configuration"),
                )),
                McpAvailabilityKind::MissingCredential => {
                    StudioMcpServerState::MissingCredential(McpMissingCredential::new(
                        snapshot
                            .availability_message
                            .as_deref()
                            .unwrap_or("MCP server credential is not configured"),
                    ))
                }
            },
            None => StudioMcpServerState::Checking(McpChecking::new("MCP health check is running")),
        },
    }
}

pub(super) fn mcp_server_checked_at(server: &StudioMcpServer) -> Option<i64> {
    match &server.state {
        StudioMcpServerState::Available(state) => Some(state.checked_at()),
        StudioMcpServerState::Unavailable(state) => Some(state.checked_at()),
        StudioMcpServerState::Disabled(_)
        | StudioMcpServerState::MissingCredential(_)
        | StudioMcpServerState::Checking(_) => None,
    }
}

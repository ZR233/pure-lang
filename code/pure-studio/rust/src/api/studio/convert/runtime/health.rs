//! agent 目录、MCP/LSP 健康与观测状态桥接。

use crate::api::studio::types::*;
use pl_studio_runtime::*;

pub(crate) fn bridge_agent_directory_entry(
    agent: StudioAgentDirectoryEntry,
) -> BridgeAgentDirectoryEntryDto {
    BridgeAgentDirectoryEntryDto {
        id: agent.id,
        thread_id: agent.thread_id,
        root_thread_id: agent.root_thread_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        summary: agent.summary,
        depth: agent.depth,
        state: bridge_agent_state(agent.state),
        progress: agent.progress.map(|progress| BridgeAgentProgressDto {
            stage: progress.stage,
            summary: progress.summary,
            next_step: progress.next_step,
            revision: progress.revision,
            updated_at: progress.updated_at,
        }),
        updated_at: agent.updated_at,
        summary_age_seconds: agent.summary_age_seconds,
    }
}

fn bridge_agent_state(state: StudioAgentState) -> BridgeAgentState {
    match state {
        StudioAgentState::Idle(_) => BridgeAgentState::Idle(BridgeIdleAgent {}),
        StudioAgentState::Queued(value) => BridgeAgentState::Queued(BridgeQueuedAgent {
            turn_id: value.turn_id().to_string(),
        }),
        StudioAgentState::Running(value) => BridgeAgentState::Running(BridgeRunningAgent {
            turn_id: value.turn_id().to_string(),
        }),
        StudioAgentState::WaitingTool(value) => {
            BridgeAgentState::WaitingTool(BridgeWaitingToolAgent {
                turn_id: value.turn_id().to_string(),
            })
        }
        StudioAgentState::WaitingInteraction(value) => {
            BridgeAgentState::WaitingInteraction(BridgeWaitingInteractionAgent {
                turn_id: value.turn_id().to_string(),
                interaction_id: value.interaction_id().to_string(),
            })
        }
        StudioAgentState::Cancelling(value) => {
            BridgeAgentState::Cancelling(BridgeCancellingAgent {
                turn_id: value.turn_id().to_string(),
            })
        }
        StudioAgentState::Closing(_) => BridgeAgentState::Closing(BridgeClosingAgent {}),
        StudioAgentState::Closed(_) => BridgeAgentState::Closed(BridgeClosedAgent {}),
        StudioAgentState::Faulted(value) => BridgeAgentState::Faulted(BridgeFaultedAgent {
            error: bridge_state_error(value.error()),
            diagnostic_turn_id: value.diagnostic_turn_id().map(ToOwned::to_owned),
        }),
    }
}

pub(crate) fn bridge_mcp_health(health: StudioMcpHealth) -> BridgeMcpHealthDto {
    BridgeMcpHealthDto {
        active_mcp_servers: health.active_mcp_servers,
        mcp_servers: health
            .mcp_servers
            .into_iter()
            .map(|server| BridgeMcpServerDto {
                id: server.id,
                transport: server.transport.to_string(),
                endpoint: server.endpoint,
                source_kind: server.source_kind,
                mutation_policy: server.mutation_policy,
                state: bridge_mcp_server_state(server.state),
            })
            .collect(),
    }
}

impl From<StudioLspHealth> for BridgeLspHealthDto {
    fn from(health: StudioLspHealth) -> Self {
        Self {
            active_lsp_servers: health.active_lsp_servers,
            lsp_servers: health
                .lsp_servers
                .into_iter()
                .map(|server| BridgeLspServerDto {
                    id: server.id,
                    display_name: server.display_name,
                    extensions: server.extensions,
                    language_ids: server.language_ids,
                    state: bridge_lsp_server_state(server.state),
                })
                .collect(),
        }
    }
}

fn bridge_mcp_server_state(state: StudioMcpServerState) -> BridgeMcpServerState {
    match state {
        StudioMcpServerState::Disabled(state) => BridgeMcpServerState::Disabled {
            message: state.message().to_string(),
        },
        StudioMcpServerState::MissingCredential(state) => BridgeMcpServerState::MissingCredential {
            message: state.message().to_string(),
        },
        StudioMcpServerState::Checking(state) => BridgeMcpServerState::Checking {
            message: state.message().to_string(),
        },
        StudioMcpServerState::Available(state) => BridgeMcpServerState::Available {
            checked_at: state.checked_at(),
            tool_count: state.tool_count(),
        },
        StudioMcpServerState::Unavailable(state) => BridgeMcpServerState::Unavailable {
            checked_at: state.checked_at(),
            error: bridge_state_error(state.error()),
        },
    }
}

fn bridge_lsp_server_state(state: StudioLspServerState) -> BridgeLspServerState {
    match state {
        StudioLspServerState::Checking(state) => BridgeLspServerState::Checking {
            message: state.message().to_string(),
        },
        StudioLspServerState::Available(state) => BridgeLspServerState::Available {
            checked_at: state.checked_at(),
            diagnostic_count: state.diagnostic_count(),
            activity: bridge_lsp_activity(state.activity()),
        },
        StudioLspServerState::Unavailable(state) => BridgeLspServerState::Unavailable {
            checked_at: state.checked_at(),
            error: bridge_state_error(state.error()),
        },
        StudioLspServerState::Disabled(state) => BridgeLspServerState::Disabled {
            message: state.message().to_string(),
        },
    }
}

fn bridge_lsp_activity(activity: &LspAvailableActivity) -> BridgeLspActivity {
    match activity {
        LspAvailableActivity::Idle(_) => BridgeLspActivity::Idle,
        LspAvailableActivity::Busy(state) => BridgeLspActivity::Busy {
            title: state.title().map(ToString::to_string),
            message: state.message().map(ToString::to_string),
            percentage: state.percentage(),
        },
        LspAvailableActivity::Indexing(state) => BridgeLspActivity::Indexing {
            title: state.title().map(ToString::to_string),
            message: state.message().map(ToString::to_string),
            percentage: state.percentage(),
        },
    }
}

pub(crate) fn bridge_state_error(error: &pl_protocol::StateError) -> BridgeStateError {
    BridgeStateError {
        code: error.code.clone(),
        message: error.message.clone(),
        retryable: error.retryable,
    }
}

pub(crate) fn bridge_uninitialized_resource(
    state: &pl_protocol::UninitializedResource,
) -> BridgeUninitializedResource {
    BridgeUninitializedResource {
        revision: state.revision(),
        updated_at: state.updated_at(),
    }
}

pub(crate) fn bridge_loading_resource(
    state: &pl_protocol::LoadingResource,
) -> BridgeLoadingResource {
    BridgeLoadingResource {
        revision: state.revision(),
        operation: state.operation().into(),
        operation_id: state.operation_id().to_string(),
        started_at: state.started_at(),
    }
}

pub(crate) fn bridge_ready_resource<T>(
    state: &pl_protocol::ReadyResource<T>,
) -> BridgeReadyResource {
    BridgeReadyResource {
        revision: state.revision(),
        updated_at: state.updated_at(),
        last_checked_at: state.last_checked_at(),
    }
}

pub(crate) fn bridge_refreshing_resource<T>(
    state: &pl_protocol::RefreshingResource<T>,
) -> BridgeRefreshingResource {
    BridgeRefreshingResource {
        revision: state.revision(),
        operation: state.operation().into(),
        operation_id: state.operation_id().to_string(),
        started_at: state.started_at(),
        last_checked_at: state.last_checked_at(),
    }
}

pub(crate) fn bridge_stale_resource<T>(
    state: &pl_protocol::StaleResource<T>,
) -> BridgeStaleResource {
    BridgeStaleResource {
        revision: state.revision(),
        stale_at: state.stale_at(),
        last_checked_at: state.last_checked_at(),
    }
}

pub(crate) fn bridge_degraded_resource<T>(
    state: &pl_protocol::DegradedResource<T>,
) -> BridgeDegradedResource {
    BridgeDegradedResource {
        revision: state.revision(),
        failed_at: state.failed_at(),
        last_checked_at: state.last_checked_at(),
        operation: state.operation().into(),
        error: state.error().clone().into(),
    }
}

pub(crate) fn bridge_failed_resource(state: &pl_protocol::FailedResource) -> BridgeFailedResource {
    BridgeFailedResource {
        revision: state.revision(),
        failed_at: state.failed_at(),
        operation: state.operation().into(),
        error: state.error().clone().into(),
    }
}

pub(crate) fn bridge_stopped_resource(
    state: &pl_protocol::StoppedResource,
) -> BridgeStoppedResource {
    BridgeStoppedResource {
        revision: state.revision(),
        stopped_at: state.stopped_at(),
    }
}

impl From<pl_protocol::StateError> for BridgeStateError {
    fn from(error: pl_protocol::StateError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        }
    }
}

impl From<pl_protocol::StateOperation> for BridgeStateOperation {
    fn from(operation: pl_protocol::StateOperation) -> Self {
        match operation {
            pl_protocol::StateOperation::Initialize => Self::Initialize,
            pl_protocol::StateOperation::Activate => Self::Activate,
            pl_protocol::StateOperation::Reload => Self::Reload,
            pl_protocol::StateOperation::Reconcile => Self::Reconcile,
            pl_protocol::StateOperation::Discover => Self::Discover,
            pl_protocol::StateOperation::Check => Self::Check,
            pl_protocol::StateOperation::Probe => Self::Probe,
            pl_protocol::StateOperation::Repair => Self::Repair,
            pl_protocol::StateOperation::Reset => Self::Reset,
            pl_protocol::StateOperation::Shutdown => Self::Shutdown,
        }
    }
}

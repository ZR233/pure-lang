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
        status: agent.status,
        summary: agent.summary,
        depth: agent.depth,
        error: agent.error,
        reason: agent.reason,
        lifecycle: agent.lifecycle,
        activity: match agent.activity {
            StudioAgentActivity::Idle => BridgeAgentActivity::Idle,
            StudioAgentActivity::Queued => BridgeAgentActivity::Queued,
            StudioAgentActivity::ActiveRunning => BridgeAgentActivity::ActiveRunning,
            StudioAgentActivity::ActiveWaitingTool => BridgeAgentActivity::ActiveWaitingTool,
            StudioAgentActivity::ActiveWaitingInteraction => {
                BridgeAgentActivity::ActiveWaitingInteraction
            }
            StudioAgentActivity::Cancelling => BridgeAgentActivity::Cancelling,
        },
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

pub(crate) fn bridge_mcp_health(health: StudioMcpHealth) -> BridgeMcpHealthDto {
    BridgeMcpHealthDto {
        active_mcp_servers: health.active_mcp_servers,
        mcp_servers: health
            .mcp_servers
            .into_iter()
            .map(|server| BridgeMcpServerDto {
                id: server.id,
                enabled: server.enabled,
                transport: server.transport.to_string(),
                endpoint: server.endpoint,
                source_kind: server.source_kind,
                status_kind: server.status_kind,
                mutation_policy: server.mutation_policy,
                availability_kind: server.availability_kind,
                availability_message: server.availability_message,
                last_checked_at: server.last_checked_at,
                tool_count: server.tool_count,
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
                    availability_kind: server.availability_kind,
                    availability_message: server.availability_message,
                    last_checked_at: server.last_checked_at,
                    diagnostic_count: server.diagnostic_count,
                    activity_kind: server.activity_kind,
                    activity_title: server.activity_title,
                    activity_message: server.activity_message,
                    activity_percentage: server.activity_percentage,
                    last_error: server.last_error,
                    last_error_at: server.last_error_at,
                })
                .collect(),
        }
    }
}

impl From<pl_protocol::ObservedStateMeta> for BridgeObservedStateMeta {
    fn from(meta: pl_protocol::ObservedStateMeta) -> Self {
        Self {
            revision: meta.revision,
            phase: meta.phase.into(),
            updated_at: meta.updated_at,
            last_checked_at: meta.last_checked_at,
            stale: meta.stale,
        }
    }
}

impl From<pl_protocol::ObservedStatePhase> for BridgeObservedStatePhase {
    fn from(phase: pl_protocol::ObservedStatePhase) -> Self {
        match phase {
            pl_protocol::ObservedStatePhase::Uninitialized => Self::Uninitialized,
            pl_protocol::ObservedStatePhase::Ready => Self::Ready,
            pl_protocol::ObservedStatePhase::Running {
                operation,
                operation_id,
            } => Self::Running {
                operation: operation.into(),
                operation_id,
            },
            pl_protocol::ObservedStatePhase::Failed { operation, error } => Self::Failed {
                operation: operation.into(),
                error: BridgeStateError {
                    code: error.code,
                    message: error.message,
                    retryable: error.retryable,
                },
            },
            pl_protocol::ObservedStatePhase::Stopped => Self::Stopped,
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

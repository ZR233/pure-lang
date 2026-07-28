use crate::api::studio::types::{
    BridgeActiveTurn, BridgeLspHealthDto, BridgeMcpHealthDto, BridgeMcpServerDto,
    BridgeRecoveryCleanupPreviewDto, BridgeRecoveryCleanupResourceDto, BridgeRecoveryIssueAction,
    BridgeRecoveryIssueCategory, BridgeRecoveryIssueScope, BridgeRecoveryResourcePresence,
    BridgeRuntimeStatus, BridgeStudioRecoveryIssueDto, BridgeTaskAgentDto, BridgeTaskMergeDto,
    BridgeTaskReviewDto, BridgeTaskRuntimeDto, BridgeTaskWorkUnitDto, RuntimeSnapshot,
};
use pl_studio_runtime::{
    StudioLspHealth, StudioMcpHealth, StudioRecoveryCleanupPreview, StudioRecoveryIssue,
    StudioRuntimeSnapshot as CoreRuntimeSnapshot,
};
// ── Core conversion functions ──

pub(crate) fn runtime_snapshot(snapshot: CoreRuntimeSnapshot) -> RuntimeSnapshot {
    RuntimeSnapshot {
        status: match snapshot.status {
            pl_studio_runtime::StudioRuntimeStatus::Uninitialized => {
                BridgeRuntimeStatus::Uninitialized
            }
            pl_studio_runtime::StudioRuntimeStatus::Initializing => {
                BridgeRuntimeStatus::Initializing
            }
            pl_studio_runtime::StudioRuntimeStatus::Ready => BridgeRuntimeStatus::Ready,
            pl_studio_runtime::StudioRuntimeStatus::ShuttingDown => {
                BridgeRuntimeStatus::ShuttingDown
            }
            pl_studio_runtime::StudioRuntimeStatus::Stopped => BridgeRuntimeStatus::Stopped,
            pl_studio_runtime::StudioRuntimeStatus::Failed => BridgeRuntimeStatus::Failed,
        },
        active_turns: snapshot
            .active_turns
            .into_iter()
            .map(|turn| BridgeActiveTurn {
                session_id: turn.session_id,
                turn_id: turn.turn_id,
            })
            .collect(),
        updated_at: snapshot.updated_at,
        error: snapshot.error,
        recovery_issues: snapshot
            .recovery_issues
            .into_iter()
            .map(bridge_recovery_issue)
            .collect(),
    }
}

pub(crate) fn bridge_recovery_issue(issue: StudioRecoveryIssue) -> BridgeStudioRecoveryIssueDto {
    BridgeStudioRecoveryIssueDto {
        id: issue.id,
        scope: bridge_recovery_issue_scope(issue.scope),
        category: match issue.category {
            pl_studio_runtime::StudioRecoveryIssueCategory::ProcessLease => {
                BridgeRecoveryIssueCategory::ProcessLease
            }
            pl_studio_runtime::StudioRecoveryIssueCategory::AgentState => {
                BridgeRecoveryIssueCategory::AgentState
            }
            pl_studio_runtime::StudioRecoveryIssueCategory::Worktree => {
                BridgeRecoveryIssueCategory::Worktree
            }
            pl_studio_runtime::StudioRecoveryIssueCategory::Repository => {
                BridgeRecoveryIssueCategory::Repository
            }
            pl_studio_runtime::StudioRecoveryIssueCategory::Merge => {
                BridgeRecoveryIssueCategory::Merge
            }
            pl_studio_runtime::StudioRecoveryIssueCategory::Conflict => {
                BridgeRecoveryIssueCategory::Conflict
            }
        },
        available_actions: vec![match issue.action {
            pl_studio_runtime::StudioRecoveryIssueAction::Retry => BridgeRecoveryIssueAction::Retry,
            pl_studio_runtime::StudioRecoveryIssueAction::CleanupSession => {
                BridgeRecoveryIssueAction::CleanupSession
            }
            pl_studio_runtime::StudioRecoveryIssueAction::RemoveProject => {
                BridgeRecoveryIssueAction::RemoveProject
            }
        }],
        project_id: issue.project_id,
        session_id: issue.session_id,
        task_run_id: issue.task_run_id,
        detail: issue.message,
    }
}

pub(crate) fn bridge_recovery_cleanup_preview(
    preview: StudioRecoveryCleanupPreview,
) -> BridgeRecoveryCleanupPreviewDto {
    BridgeRecoveryCleanupPreviewDto {
        issue_id: preview.issue_id,
        expected_revision: preview.expected_revision,
        scope: bridge_recovery_issue_scope(preview.scope),
        project_id: preview.project_id,
        session_id: preview.session_id,
        detail: preview.message,
        resources: preview
            .resources
            .into_iter()
            .map(|resource| BridgeRecoveryCleanupResourceDto {
                work_unit_id: resource.work_unit_id,
                path: resource.path,
                branch: resource.branch,
                presence: match resource.presence {
                    pl_studio_runtime::StudioRecoveryResourcePresence::Absent => {
                        BridgeRecoveryResourcePresence::Absent
                    }
                    pl_studio_runtime::StudioRecoveryResourcePresence::Complete => {
                        BridgeRecoveryResourcePresence::Complete
                    }
                    pl_studio_runtime::StudioRecoveryResourcePresence::Partial => {
                        BridgeRecoveryResourcePresence::Partial
                    }
                },
                registration_exists: resource.registration_exists,
                path_exists: resource.path_exists,
                branch_exists: resource.branch_exists,
                branch_head: resource.branch_head,
                dirty: resource.dirty,
                ahead_by: resource.ahead_by,
                changed_file_count: resource.changed_file_count,
            })
            .collect(),
    }
}

fn bridge_recovery_issue_scope(
    scope: pl_studio_runtime::StudioRecoveryIssueScope,
) -> BridgeRecoveryIssueScope {
    match scope {
        pl_studio_runtime::StudioRecoveryIssueScope::Application => {
            BridgeRecoveryIssueScope::Application
        }
        pl_studio_runtime::StudioRecoveryIssueScope::Project => BridgeRecoveryIssueScope::Project,
        pl_studio_runtime::StudioRecoveryIssueScope::Session => BridgeRecoveryIssueScope::Session,
    }
}

pub(crate) fn bridge_task_runtime(
    task: pl_studio_runtime::StudioTaskRuntime,
) -> BridgeTaskRuntimeDto {
    BridgeTaskRuntimeDto {
        run_id: task.run_id,
        phase: task.phase,
        branch: task.branch,
        expected_head: task.expected_head,
        status_message: task.status_message,
        work_units: task
            .work_units
            .into_iter()
            .map(|unit| BridgeTaskWorkUnitDto {
                id: unit.id,
                title: unit.title,
                status: unit.status,
                worktree_path: unit.worktree_path,
                branch: unit.branch,
                agent_id: unit.agent_id,
            })
            .collect(),
        agents: task
            .agents
            .into_iter()
            .map(|agent| BridgeTaskAgentDto {
                agent_id: agent.agent_id,
                role: agent.role,
                status: agent.status,
                initiated_by: agent.initiated_by,
                requested_by_call_id: agent.requested_by_call_id,
                summary: agent.summary,
                error: agent.error,
                head_commit: agent.head_commit,
            })
            .collect(),
        merges: task
            .merges
            .into_iter()
            .map(|merge| BridgeTaskMergeDto {
                id: merge.id,
                agent_id: merge.agent_id,
                status: merge.status,
                merge_commit: merge.merge_commit,
                conflict_files: merge.conflict_files,
                resolution_summary: merge.resolution_summary,
            })
            .collect(),
        reviews: task
            .reviews
            .into_iter()
            .map(|review| BridgeTaskReviewDto {
                round: review.round,
                head_commit: review.head_commit,
                verdict: review.verdict,
                reviewer_agent_id: review.reviewer_agent_id,
                summary: review.summary,
                design_references: review.design_references,
            })
            .collect(),
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
                command: server.command,
                url: server.url,
                endpoint: server.endpoint,
                source_kind: server.source_kind,
                status_kind: server.status_kind,
                mutation_policy: server.mutation_policy,
                availability_kind: server.availability_kind,
            })
            .collect(),
    }
}

pub(crate) fn bridge_lsp_health(health: StudioLspHealth) -> BridgeLspHealthDto {
    BridgeLspHealthDto {
        active_lsp_servers: health.active_lsp_servers,
    }
}

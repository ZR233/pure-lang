use crate::api::studio::types::{
    BridgeActiveTurn, BridgeAgentDirectoryEntryDto, BridgeAgentProgressDto, BridgeLspHealthDto,
    BridgeMcpHealthDto, BridgeMcpServerDto, BridgeRecoveryCleanupPreviewDto,
    BridgeRecoveryCleanupResourceDto, BridgeRecoveryIssueAction, BridgeRecoveryIssueCategory,
    BridgeRecoveryIssueScope, BridgeRecoveryResourcePresence, BridgeRuntimeStatus,
    BridgeStudioRecoveryIssueDto, BridgeTaskCompletionDto, BridgeTaskDesignReferenceDto,
    BridgeTaskMergeDto, BridgeTaskReviewDto, BridgeTaskReviewFindingDto, BridgeTaskRuntimeDto,
    BridgeTaskWorkUnitDto, RuntimeSnapshot,
};
use pl_studio_runtime::{
    StudioAgentDirectoryEntry, StudioLspHealth, StudioMcpHealth, StudioRecoveryCleanupPreview,
    StudioRecoveryIssue, StudioRuntimeSnapshot as CoreRuntimeSnapshot,
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
                thread_id: turn.thread_id,
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
            pl_studio_runtime::StudioRecoveryIssueAction::CleanupThread => {
                BridgeRecoveryIssueAction::CleanupThread
            }
            pl_studio_runtime::StudioRecoveryIssueAction::RemoveProject => {
                BridgeRecoveryIssueAction::RemoveProject
            }
        }],
        project_id: issue.project_id,
        thread_id: issue.thread_id,
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
        thread_id: preview.thread_id,
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
        pl_studio_runtime::StudioRecoveryIssueScope::Thread => BridgeRecoveryIssueScope::Thread,
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
        stop_requested_origin: task.stop_requested_origin,
        stop_requested_reason: task.stop_requested_reason,
        task_generation: task.task_generation,
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
        completions: task
            .completions
            .into_iter()
            .map(|completion| BridgeTaskCompletionDto {
                id: completion.id,
                work_unit_id: completion.work_unit_id,
                executor_agent_id: completion.executor_agent_id,
                revision: completion.revision,
                kind: completion.kind,
                status: completion.status,
                base_commit: completion.base_commit,
                head_commit: completion.head_commit,
                changed_files: completion.changed_files,
                verification_summary: completion.verification_summary,
                worktree_path: completion.worktree_path,
                branch: completion.branch,
                created_at: completion.created_at,
                updated_at: completion.updated_at,
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
                id: review.id,
                round: review.round,
                scope: review.scope,
                work_unit_id: review.work_unit_id,
                completion_id: review.completion_id,
                completion_revision: review.completion_revision,
                reviewed_head: review.reviewed_head,
                verdict: review.verdict,
                requested_by_call_id: review.requested_by_call_id,
                reviewer_agent_id: review.reviewer_agent_id,
                summary: review.summary,
                design_references: review
                    .design_references
                    .into_iter()
                    .map(|reference| BridgeTaskDesignReferenceDto {
                        path: reference.path,
                        section: reference.section,
                    })
                    .collect(),
                findings: review
                    .findings
                    .into_iter()
                    .map(|finding| BridgeTaskReviewFindingDto {
                        severity: finding.severity,
                        title: finding.title,
                        body: finding.body,
                        path: finding.path,
                        line: finding.line,
                        design_references: finding
                            .design_references
                            .into_iter()
                            .map(|reference| BridgeTaskDesignReferenceDto {
                                path: reference.path,
                                section: reference.section,
                            })
                            .collect(),
                    })
                    .collect(),
                created_at: review.created_at,
                updated_at: review.updated_at,
            })
            .collect(),
    }
}

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
        activity: agent.activity,
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

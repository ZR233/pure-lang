//! 恢复问题与 Task 会话恢复桥接。

use crate::api::studio::types::*;
use pl_protocol::ConversationRecoveryMode;
use pl_studio_runtime::*;

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

pub(crate) fn bridge_task_recovery_preview(
    preview: StudioTaskRecoveryPreview,
) -> BridgeTaskRecoveryPreviewDto {
    BridgeTaskRecoveryPreviewDto {
        preview_token: preview.preview_token,
        root_thread_id: preview.root_thread_id,
        run_id: preview.run_id,
        revision: preview.revision,
        task_generation: preview.task_generation,
        state: bridge_task_recovery_state(preview.state),
        stop_requested: preview.stop_requested,
        project_lease_id: preview.project_lease_id,
        recommended_thread_id: preview.recommended_thread_id,
        targets: preview
            .targets
            .into_iter()
            .map(bridge_task_recovery_target)
            .collect(),
        completion_revision_fingerprint: preview.completion_revision_fingerprint,
        review_revision_fingerprint: preview.review_revision_fingerprint,
        merge_revision_fingerprint: preview.merge_revision_fingerprint,
    }
}

const fn bridge_task_recovery_state(
    state: pl_protocol::studio::StudioTaskRecoveryState,
) -> BridgeTaskRecoveryState {
    use pl_protocol::studio::StudioTaskRecoveryState as Source;
    match state {
        Source::DesignUpdating => BridgeTaskRecoveryState::DesignUpdating,
        Source::Implementing => BridgeTaskRecoveryState::Implementing,
        Source::Merging => BridgeTaskRecoveryState::Merging,
        Source::Reviewing => BridgeTaskRecoveryState::Reviewing,
        Source::Reworking => BridgeTaskRecoveryState::Reworking,
        Source::Stopping => BridgeTaskRecoveryState::Stopping,
        Source::Blocked => BridgeTaskRecoveryState::Blocked,
        Source::Completed => BridgeTaskRecoveryState::Completed,
        Source::Failed => BridgeTaskRecoveryState::Failed,
        Source::Cancelled => BridgeTaskRecoveryState::Cancelled,
    }
}

const fn protocol_task_recovery_state(
    state: BridgeTaskRecoveryState,
) -> pl_protocol::studio::StudioTaskRecoveryState {
    use pl_protocol::studio::StudioTaskRecoveryState as Target;
    match state {
        BridgeTaskRecoveryState::DesignUpdating => Target::DesignUpdating,
        BridgeTaskRecoveryState::Implementing => Target::Implementing,
        BridgeTaskRecoveryState::Merging => Target::Merging,
        BridgeTaskRecoveryState::Reviewing => Target::Reviewing,
        BridgeTaskRecoveryState::Reworking => Target::Reworking,
        BridgeTaskRecoveryState::Stopping => Target::Stopping,
        BridgeTaskRecoveryState::Blocked => Target::Blocked,
        BridgeTaskRecoveryState::Completed => Target::Completed,
        BridgeTaskRecoveryState::Failed => Target::Failed,
        BridgeTaskRecoveryState::Cancelled => Target::Cancelled,
    }
}

pub(crate) fn task_recovery_request(
    request: BridgeTaskRecoveryRequestDto,
) -> StudioTaskRecoveryRequest {
    StudioTaskRecoveryRequest {
        recovery_id: request.recovery_id,
        root_thread_id: request.root_thread_id,
        target_thread_id: request.target_thread_id,
        mode: request.mode.into(),
        turn_ids: request.turn_ids,
        preview: task_recovery_preview_from_bridge(request.preview),
    }
}

pub(crate) fn bridge_task_recovery_result(
    result: StudioTaskRecoveryResult,
) -> BridgeTaskRecoveryResultDto {
    BridgeTaskRecoveryResultDto {
        recovery_id: result.recovery_id,
        run_id: result.run_id,
        work_unit_id: result.work_unit_id,
        root_thread_id: result.root_thread_id,
        target_thread_id: result.target_thread_id,
        mode: result.mode.into(),
        recovery_revision: result.recovery_revision,
        runtime_revision: result.runtime_revision,
        thread_revision: result.thread_revision,
        before_transcript_hash: result.before_transcript_hash,
        after_transcript_hash: result.after_transcript_hash,
        removed_item_count: result.removed_item_count,
        removed_input_count: result.removed_input_count,
        stop_cleared: result.stop_cleared,
        resume_turn_id: result.resume_turn_id,
    }
}

fn bridge_task_recovery_target(target: StudioTaskRecoveryTarget) -> BridgeTaskRecoveryTargetDto {
    BridgeTaskRecoveryTargetDto {
        thread_id: target.thread_id,
        kind: match target.kind {
            StudioTaskRecoveryTargetKind::Planner => BridgeTaskRecoveryTargetKind::Planner,
            StudioTaskRecoveryTargetKind::Executor => BridgeTaskRecoveryTargetKind::Executor,
        },
        work_unit_id: target.work_unit_id,
        attempt: target.attempt,
        continuation_revision: target.continuation_revision,
        expected_runtime_revision: target.expected_runtime_revision,
        expected_thread_revision: target.expected_thread_revision,
        branch: target.branch,
        worktree_path: target.worktree_path,
        base_commit: target.base_commit,
        turns: target
            .turns
            .into_iter()
            .map(|turn| BridgeTaskRecoveryTurnDto {
                turn_id: turn.turn_id,
                status: turn.status,
                updated_at: turn.updated_at,
                item_count: turn.item_count,
                input_count: turn.input_count,
                tool_count: turn.tool_count,
                tool_summaries: turn.tool_summaries,
            })
            .collect(),
        default_turn_ids: target.default_turn_ids,
        available_modes: target
            .available_modes
            .into_iter()
            .map(BridgeConversationRecoveryMode::from)
            .collect(),
    }
}

fn task_recovery_preview_from_bridge(
    preview: BridgeTaskRecoveryPreviewDto,
) -> StudioTaskRecoveryPreview {
    StudioTaskRecoveryPreview {
        preview_token: preview.preview_token,
        root_thread_id: preview.root_thread_id,
        run_id: preview.run_id,
        revision: preview.revision,
        task_generation: preview.task_generation,
        state: protocol_task_recovery_state(preview.state),
        stop_requested: preview.stop_requested,
        project_lease_id: preview.project_lease_id,
        recommended_thread_id: preview.recommended_thread_id,
        targets: preview
            .targets
            .into_iter()
            .map(|target| StudioTaskRecoveryTarget {
                thread_id: target.thread_id,
                kind: match target.kind {
                    BridgeTaskRecoveryTargetKind::Planner => StudioTaskRecoveryTargetKind::Planner,
                    BridgeTaskRecoveryTargetKind::Executor => {
                        StudioTaskRecoveryTargetKind::Executor
                    }
                },
                work_unit_id: target.work_unit_id,
                attempt: target.attempt,
                continuation_revision: target.continuation_revision,
                expected_runtime_revision: target.expected_runtime_revision,
                expected_thread_revision: target.expected_thread_revision,
                branch: target.branch,
                worktree_path: target.worktree_path,
                base_commit: target.base_commit,
                turns: target
                    .turns
                    .into_iter()
                    .map(|turn| StudioTaskRecoveryTurn {
                        turn_id: turn.turn_id,
                        status: turn.status,
                        updated_at: turn.updated_at,
                        item_count: turn.item_count,
                        input_count: turn.input_count,
                        tool_count: turn.tool_count,
                        tool_summaries: turn.tool_summaries,
                    })
                    .collect(),
                default_turn_ids: target.default_turn_ids,
                available_modes: target
                    .available_modes
                    .into_iter()
                    .map(ConversationRecoveryMode::from)
                    .collect(),
            })
            .collect(),
        completion_revision_fingerprint: preview.completion_revision_fingerprint,
        review_revision_fingerprint: preview.review_revision_fingerprint,
        merge_revision_fingerprint: preview.merge_revision_fingerprint,
    }
}

impl From<ConversationRecoveryMode> for BridgeConversationRecoveryMode {
    fn from(mode: ConversationRecoveryMode) -> Self {
        match mode {
            ConversationRecoveryMode::RewindTail => Self::RewindTail,
            ConversationRecoveryMode::RebuildThread => Self::RebuildThread,
        }
    }
}

impl From<BridgeConversationRecoveryMode> for ConversationRecoveryMode {
    fn from(mode: BridgeConversationRecoveryMode) -> Self {
        match mode {
            BridgeConversationRecoveryMode::RewindTail => Self::RewindTail,
            BridgeConversationRecoveryMode::RebuildThread => Self::RebuildThread,
        }
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

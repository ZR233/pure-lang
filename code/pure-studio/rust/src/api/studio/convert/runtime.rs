use crate::api::studio::types::*;
use pl_protocol::ConversationRecoveryMode;
use pl_studio_runtime::*;
// ── Core conversion functions ──

/// 把 core lifecycle 快照转换成 FRB `RuntimeSnapshot`。
///
/// UI 从 per-thread stream 派生活动 turn，并从 Studio bootstrap/snapshot 响应读取
/// 恢复问题；lifecycle 快照只承载 lifecycle 本身。
pub(crate) fn runtime_snapshot(snapshot: StudioRuntimeSnapshot) -> RuntimeSnapshot {
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
        updated_at: snapshot.updated_at,
        error: snapshot.error,
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

pub(crate) fn bridge_task_recovery_preview(
    preview: StudioTaskRecoveryPreview,
) -> BridgeTaskRecoveryPreviewDto {
    BridgeTaskRecoveryPreviewDto {
        preview_token: preview.preview_token,
        root_thread_id: preview.root_thread_id,
        run_id: preview.run_id,
        task_generation: preview.task_generation,
        phase: preview.phase,
        expected_head: preview.expected_head,
        stop_requested: preview.stop_requested,
        branch_lease_id: preview.branch_lease_id,
        branch_lease_branch: preview.branch_lease_branch,
        branch_lease_git_common_dir: preview.branch_lease_git_common_dir,
        branch_lease_expected_head: preview.branch_lease_expected_head,
        recommended_thread_id: preview.recommended_thread_id,
        targets: preview
            .targets
            .into_iter()
            .map(bridge_task_recovery_target)
            .collect(),
        main_git_fingerprint: preview.main_git_fingerprint.into(),
        completion_revision_fingerprint: preview.completion_revision_fingerprint,
        review_revision_fingerprint: preview.review_revision_fingerprint,
        merge_revision_fingerprint: preview.merge_revision_fingerprint,
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
        git_fingerprint: result.git_fingerprint.into(),
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
        git_fingerprint: target.git_fingerprint.into(),
    }
}

impl From<StudioTaskGitFingerprint> for BridgeTaskGitFingerprintDto {
    fn from(fingerprint: StudioTaskGitFingerprint) -> Self {
        Self {
            workspace_root: fingerprint.workspace_root,
            git_common_dir: fingerprint.git_common_dir,
            branch: fingerprint.branch,
            head: fingerprint.head,
            base_commit: fingerprint.base_commit,
            expected_head: fingerprint.expected_head,
            operation: fingerprint.operation,
            index_diff_hash: fingerprint.index_diff_hash,
            working_tree_diff_hash: fingerprint.working_tree_diff_hash,
            untracked_content_hash: fingerprint.untracked_content_hash,
        }
    }
}

fn task_recovery_preview_from_bridge(
    preview: BridgeTaskRecoveryPreviewDto,
) -> StudioTaskRecoveryPreview {
    StudioTaskRecoveryPreview {
        preview_token: preview.preview_token,
        root_thread_id: preview.root_thread_id,
        run_id: preview.run_id,
        task_generation: preview.task_generation,
        phase: preview.phase,
        expected_head: preview.expected_head,
        stop_requested: preview.stop_requested,
        branch_lease_id: preview.branch_lease_id,
        branch_lease_branch: preview.branch_lease_branch,
        branch_lease_git_common_dir: preview.branch_lease_git_common_dir,
        branch_lease_expected_head: preview.branch_lease_expected_head,
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
                git_fingerprint: target.git_fingerprint.into(),
            })
            .collect(),
        main_git_fingerprint: preview.main_git_fingerprint.into(),
        completion_revision_fingerprint: preview.completion_revision_fingerprint,
        review_revision_fingerprint: preview.review_revision_fingerprint,
        merge_revision_fingerprint: preview.merge_revision_fingerprint,
    }
}

impl From<BridgeTaskGitFingerprintDto> for StudioTaskGitFingerprint {
    fn from(fingerprint: BridgeTaskGitFingerprintDto) -> Self {
        Self {
            workspace_root: fingerprint.workspace_root,
            git_common_dir: fingerprint.git_common_dir,
            branch: fingerprint.branch,
            head: fingerprint.head,
            base_commit: fingerprint.base_commit,
            expected_head: fingerprint.expected_head,
            operation: fingerprint.operation,
            index_diff_hash: fingerprint.index_diff_hash,
            working_tree_diff_hash: fingerprint.working_tree_diff_hash,
            untracked_content_hash: fingerprint.untracked_content_hash,
        }
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
        failures: task.failures.into_iter().map(bridge_task_failure).collect(),
        terminal_failure: task.terminal_failure.map(bridge_task_failure),
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
                execution_status: unit.execution_status,
                execution_error: unit.execution_error,
                budget_limit: unit.budget_limit.map(|limit| BridgeBudgetLimitDto {
                    kind: limit.kind,
                    usage: BridgeBudgetUsageDto {
                        model_steps: limit.usage.model_steps,
                        tool_calls: limit.usage.tool_calls,
                        wait_calls: limit.usage.wait_calls,
                        elapsed_ms: limit.usage.elapsed_ms,
                    },
                }),
                budget_slice_count: unit.budget_slice_count,
                budget_slice_limit: unit.budget_slice_limit,
                continuation_state: unit.continuation_state,
                continuation_source_turn_id: unit.continuation_source_turn_id,
                continuation_revision: unit.continuation_revision,
                executor_progress_revision: unit.executor_progress_revision,
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
                work_unit_id: merge.work_unit_id,
                completion_id: merge.completion_id,
                completion_revision: merge.completion_revision,
                executor_agent_id: merge.executor_agent_id,
                expected_previous_head: merge.expected_previous_head,
                resulting_head: merge.resulting_head,
                delivery_head: merge.delivery_head,
                method: merge.method,
                summary: merge.summary,
                cleanup_status: merge.cleanup_status,
                cleanup_detail: merge.cleanup_detail,
                created_at: merge.created_at,
                updated_at: merge.updated_at,
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
                        recommendation: finding.recommendation,
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

fn bridge_task_failure(
    failure: pl_studio_runtime::StudioTaskFailureRuntime,
) -> super::super::types::BridgeTaskFailureDto {
    super::super::types::BridgeTaskFailureDto {
        id: failure.id,
        source_thread_id: failure.source_thread_id,
        source_turn_id: failure.source_turn_id,
        source_agent_id: failure.source_agent_id,
        source_role: failure.source_role,
        work_unit_id: failure.work_unit_id,
        review_round_id: failure.review_round_id,
        disposition: failure.disposition,
        category: format!("{:?}", failure.failure.category).to_ascii_lowercase(),
        provider_kind: failure
            .failure
            .provider_kind
            .map(|kind| format!("{kind:?}").to_ascii_lowercase()),
        code: failure.failure.code,
        http_status: failure.failure.http_status,
        message: failure.failure.message,
        retryable: failure.failure.retry.is_retryable(),
        resolved_at: failure.resolved_at,
        created_at: failure.created_at,
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

impl From<StudioLspHealth> for BridgeLspHealthDto {
    fn from(health: StudioLspHealth) -> Self {
        Self {
            active_lsp_servers: health.active_lsp_servers,
        }
    }
}

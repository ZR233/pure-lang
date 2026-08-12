use anyhow::Result;

use crate::{
    StudioBudgetLimitRuntime, StudioBudgetUsageRuntime, StudioTaskCompletionRuntime,
    StudioTaskDesignReferenceRuntime, StudioTaskFailureRuntime, StudioTaskMergeRuntime,
    StudioTaskReviewFindingRuntime, StudioTaskReviewRuntime, StudioTaskRuntime,
    StudioTaskWorkUnitRuntime,
};

use super::{
    StudioStore,
    task_coordinator::{MergeRecord, ReviewRoundRecord, TaskRunRecord, WorkCompletionRecord},
};

pub(crate) async fn load_task_runtime(
    store: &StudioStore,
    root_thread_id: &str,
) -> Result<Option<StudioTaskRuntime>> {
    let Some(run) = store
        .find_latest_task_run_for_root_thread(root_thread_id)
        .await?
    else {
        return Ok(None);
    };
    let work_units = store.list_work_units(&run.id).await?;
    let mut work_unit_runtimes = Vec::with_capacity(work_units.len());
    for unit in work_units {
        let executor_progress_revision = if let Some(executor_thread_id) = &unit.executor_thread_id
        {
            store
                .read_thread_runtime_revision(executor_thread_id)
                .await?
        } else {
            0
        };
        work_unit_runtimes.push(StudioTaskWorkUnitRuntime {
            id: unit.id,
            title: unit.title,
            status: unit.status.as_str().to_string(),
            worktree_path: unit.worktree_path,
            branch: unit.branch,
            agent_id: unit.executor_thread_id,
            execution_status: unit.execution_status.as_str().to_string(),
            execution_error: unit.execution_error,
            budget_limit: unit.budget_limit.map(|limit| StudioBudgetLimitRuntime {
                kind: limit.kind.as_str().to_string(),
                usage: StudioBudgetUsageRuntime {
                    model_steps: limit.usage.model_steps,
                    tool_calls: limit.usage.tool_calls,
                    wait_calls: limit.usage.wait_calls,
                    elapsed_ms: limit.usage.elapsed_ms,
                },
            }),
            budget_slice_count: unit.budget_slice_count,
            budget_slice_limit: crate::studio::task_coordinator::MAX_EXECUTOR_BUDGET_SLICES,
            continuation_state: unit.continuation_state.as_str().to_string(),
            continuation_source_turn_id: unit.continuation_source_turn_id,
            continuation_revision: unit.continuation_revision,
            executor_progress_revision,
        });
    }
    let completions = store.list_work_completions(&run.id).await?;
    let merges = store.list_merge_records(&run.id).await?;
    let reviews = store.list_review_rounds(&run.id).await?;
    let failures = store.list_task_failures(&run.id).await?;
    Ok(Some(studio_task_runtime(
        run,
        work_unit_runtimes,
        completions,
        merges,
        reviews,
        failures,
    )))
}

fn studio_task_runtime(
    run: TaskRunRecord,
    work_units: Vec<StudioTaskWorkUnitRuntime>,
    completions: Vec<WorkCompletionRecord>,
    merges: Vec<MergeRecord>,
    reviews: Vec<ReviewRoundRecord>,
    failures: Vec<super::task_coordinator::TaskFailureRecord>,
) -> StudioTaskRuntime {
    let terminal_failure_id = run.terminal_failure_id.clone();
    let all_failures = failures
        .into_iter()
        .map(|failure| StudioTaskFailureRuntime {
            id: failure.id,
            source_thread_id: failure.source_thread_id,
            source_turn_id: failure.source_turn_id,
            source_agent_id: failure.source_agent_id,
            source_role: failure.source_role,
            work_unit_id: failure.work_unit_id,
            review_round_id: failure.review_round_id,
            disposition: failure.disposition.as_str().to_string(),
            failure: failure.failure,
            resolved_at: failure.resolved_at,
            created_at: failure.created_at,
        })
        .collect::<Vec<_>>();
    let terminal_failure = terminal_failure_id
        .as_deref()
        .and_then(|id| all_failures.iter().find(|failure| failure.id == id))
        .cloned();
    let failures = all_failures
        .into_iter()
        .filter(|failure| failure.resolved_at.is_none())
        .collect();
    StudioTaskRuntime {
        run_id: run.id,
        phase: run.phase.as_str().to_string(),
        branch: run.branch,
        expected_head: run.expected_head,
        status_message: run.status_message,
        stop_requested_origin: run
            .stop_requested_origin
            .map(|origin| origin.as_str().to_string()),
        stop_requested_reason: run
            .stop_requested_reason
            .map(|reason| reason.as_str().to_string()),
        task_generation: run.task_generation,
        failures,
        terminal_failure,
        work_units,
        completions: completions
            .into_iter()
            .map(|completion| StudioTaskCompletionRuntime {
                id: completion.id,
                work_unit_id: completion.work_unit_id,
                executor_agent_id: completion.executor_agent_id,
                revision: completion.revision,
                kind: completion.kind.as_str().to_string(),
                status: completion.status.as_str().to_string(),
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
        merges: merges
            .into_iter()
            .map(|merge| StudioTaskMergeRuntime {
                id: merge.id,
                work_unit_id: merge.work_unit_id,
                completion_id: merge.completion_id,
                completion_revision: merge.completion_revision,
                executor_agent_id: merge.executor_agent_id,
                expected_previous_head: merge.expected_previous_head,
                resulting_head: merge.resulting_head,
                delivery_head: merge.delivery_head,
                method: merge.method.as_str().to_string(),
                summary: merge.summary,
                cleanup_status: merge.cleanup.status,
                cleanup_detail: merge.cleanup.detail,
                created_at: merge.created_at,
                updated_at: merge.updated_at,
            })
            .collect(),
        reviews: reviews
            .into_iter()
            .map(|review| StudioTaskReviewRuntime {
                id: review.id,
                round: review.round,
                scope: review.scope.as_str().to_string(),
                work_unit_id: review.work_unit_id,
                completion_id: review.completion_id,
                completion_revision: review.completion_revision,
                reviewed_head: review.reviewed_head,
                verdict: review.verdict.as_str().to_string(),
                requested_by_call_id: review.requested_by_call_id,
                reviewer_agent_id: review.reviewer_thread_id,
                summary: review.summary,
                design_references: review
                    .design_references
                    .into_iter()
                    .map(|reference| StudioTaskDesignReferenceRuntime {
                        path: reference.path,
                        section: reference.section,
                    })
                    .collect(),
                findings: review
                    .findings
                    .into_iter()
                    .map(|finding| StudioTaskReviewFindingRuntime {
                        severity: finding.severity,
                        title: finding.title,
                        body: finding.body,
                        recommendation: finding.recommendation,
                        path: finding.path,
                        line: finding.line,
                        design_references: finding
                            .design_references
                            .into_iter()
                            .map(|reference| StudioTaskDesignReferenceRuntime {
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

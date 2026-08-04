use anyhow::Result;

use crate::{
    StudioTaskCompletionRuntime, StudioTaskDesignReferenceRuntime, StudioTaskMergeRuntime,
    StudioTaskReviewFindingRuntime, StudioTaskReviewRuntime, StudioTaskRuntime,
    StudioTaskWorkUnitRuntime,
};

use super::{
    StudioStore,
    task_coordinator::{
        MergeRecord, ReviewRoundRecord, TaskRunRecord, WorkCompletionRecord, WorkUnitRecord,
    },
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
    Ok(Some(studio_task_runtime(
        run.clone(),
        store.list_work_units(&run.id).await?,
        store.list_work_completions(&run.id).await?,
        store.list_merge_records(&run.id).await?,
        store.list_review_rounds(&run.id).await?,
    )))
}

fn studio_task_runtime(
    run: TaskRunRecord,
    work_units: Vec<WorkUnitRecord>,
    completions: Vec<WorkCompletionRecord>,
    merges: Vec<MergeRecord>,
    reviews: Vec<ReviewRoundRecord>,
) -> StudioTaskRuntime {
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
        work_units: work_units
            .iter()
            .map(|unit| StudioTaskWorkUnitRuntime {
                id: unit.id.clone(),
                title: unit.title.clone(),
                status: unit.status.as_str().to_string(),
                worktree_path: unit.worktree_path.clone(),
                branch: unit.branch.clone(),
                agent_id: unit.executor_thread_id.clone(),
            })
            .collect(),
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
                agent_id: merge.agent_id,
                status: merge.status.as_str().to_string(),
                merge_commit: merge
                    .evidence
                    .as_ref()
                    .and_then(|evidence| evidence.merge_commit.clone()),
                conflict_files: merge.conflict_files,
                resolution_summary: merge.resolution_summary,
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

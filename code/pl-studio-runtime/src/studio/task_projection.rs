use anyhow::Result;

use crate::{
    StudioTaskAgentRuntime, StudioTaskMergeRuntime, StudioTaskReviewRuntime, StudioTaskRuntime,
    StudioTaskWorkUnitRuntime,
};

use super::{
    StudioStore,
    task_coordinator::{
        AgentOutcomeRecord, MergeRecord, ReviewRoundRecord, TaskRunRecord, WorkUnitRecord,
    },
};

pub(crate) async fn load_task_runtime(
    store: &StudioStore,
    session_id: &str,
) -> Result<Option<StudioTaskRuntime>> {
    let Some(run) = store.find_latest_task_run_for_session(session_id).await? else {
        return Ok(None);
    };
    Ok(Some(studio_task_runtime(
        run.clone(),
        store.list_work_units(&run.id).await?,
        store.list_agent_outcomes(&run.id).await?,
        store.list_merge_records(&run.id).await?,
        store.list_review_rounds(&run.id).await?,
    )))
}

fn studio_task_runtime(
    run: TaskRunRecord,
    work_units: Vec<WorkUnitRecord>,
    agents: Vec<AgentOutcomeRecord>,
    merges: Vec<MergeRecord>,
    reviews: Vec<ReviewRoundRecord>,
) -> StudioTaskRuntime {
    StudioTaskRuntime {
        run_id: run.id,
        phase: run.phase.as_str().to_string(),
        branch: run.branch,
        expected_head: run.expected_head,
        status_message: run.status_message,
        work_units: work_units
            .into_iter()
            .map(|unit| StudioTaskWorkUnitRuntime {
                id: unit.id,
                title: unit.title,
                status: unit.status.as_str().to_string(),
                worktree_path: unit.worktree_path,
                branch: unit.branch,
                agent_id: unit.agent_id,
            })
            .collect(),
        agents: agents
            .into_iter()
            .map(|agent| StudioTaskAgentRuntime {
                agent_id: agent.agent_id,
                role: agent.role,
                status: agent.status.as_str().to_string(),
                initiated_by: agent.initiated_by,
                requested_by_call_id: agent.requested_by_call_id,
                summary: agent.summary,
                error: agent.error,
                head_commit: agent.delivery.map(|delivery| delivery.head_commit),
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
                round: review.round,
                head_commit: review.head_commit,
                verdict: review.verdict.as_str().to_string(),
                reviewer_agent_id: review.reviewer_agent_id,
                summary: review.summary,
                design_references: review
                    .design_references
                    .into_iter()
                    .map(|reference| format!("{}#{}", reference.path, reference.section))
                    .collect(),
            })
            .collect(),
    }
}

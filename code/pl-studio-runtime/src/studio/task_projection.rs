use std::collections::HashMap;

use anyhow::Result;
use pl_core::{
    AgentActivityState, AgentLifecycleState, AgentProgressCheckpoint, AgentProgressStage,
    AgentSnapshot,
};

use crate::{
    StudioAgentProgressRuntime, StudioTaskAgentRuntime, StudioTaskCompletionRuntime,
    StudioTaskDesignReferenceRuntime, StudioTaskMergeRuntime, StudioTaskReviewFindingRuntime,
    StudioTaskReviewRuntime, StudioTaskRuntime, StudioTaskWorkUnitRuntime,
};

use super::{
    StudioStore,
    ids::unix_seconds,
    task_coordinator::{
        AgentOutcomeRecord, MergeRecord, ReviewRoundRecord, TaskRunRecord, WorkCompletionRecord,
        WorkUnitRecord,
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
        store.list_work_completions(&run.id).await?,
        store.list_merge_records(&run.id).await?,
        store.list_review_rounds(&run.id).await?,
        store.list_task_agent_snapshots(&run.id).await?,
    )))
}

fn studio_task_runtime(
    run: TaskRunRecord,
    work_units: Vec<WorkUnitRecord>,
    agents: Vec<AgentOutcomeRecord>,
    completions: Vec<WorkCompletionRecord>,
    merges: Vec<MergeRecord>,
    reviews: Vec<ReviewRoundRecord>,
    snapshots: Vec<AgentSnapshot>,
) -> StudioTaskRuntime {
    let completion_heads = completions
        .iter()
        .filter_map(|completion| {
            completion.head_commit.as_ref().map(|head| {
                (
                    completion.executor_agent_id.clone(),
                    (completion.revision, head.clone()),
                )
            })
        })
        .fold(
            HashMap::<String, (u32, String)>::new(),
            |mut heads, (agent_id, candidate)| {
                if heads
                    .get(&agent_id)
                    .is_none_or(|current| candidate.0 > current.0)
                {
                    heads.insert(agent_id, candidate);
                }
                heads
            },
        );
    let snapshots = snapshots
        .into_iter()
        .map(|snapshot| (snapshot.identity.id.to_string(), snapshot))
        .collect::<HashMap<_, _>>();
    let now = unix_seconds();
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
            .map(|agent| {
                let head_commit = completion_heads
                    .get(&agent.agent_id)
                    .map(|(_, head)| head.clone());
                let snapshot = snapshots.get(&agent.agent_id);
                StudioTaskAgentRuntime {
                    lifecycle: snapshot
                        .map(|snapshot| lifecycle_label(snapshot.lifecycle).to_string()),
                    activity: snapshot
                        .map(|snapshot| activity_label(snapshot.activity).to_string()),
                    progress: snapshot
                        .and_then(|snapshot| snapshot.progress.as_ref())
                        .map(progress_runtime),
                    updated_at: snapshot.map_or(agent.updated_at, |snapshot| snapshot.updated_at),
                    summary_age_seconds: summary_age_seconds(snapshot, agent.updated_at, now),
                    agent_id: agent.agent_id,
                    role: agent.role,
                    status: agent.status.as_str().to_string(),
                    initiated_by: agent.initiated_by,
                    requested_by_call_id: agent.requested_by_call_id,
                    summary: agent.summary,
                    error: agent.error,
                    head_commit,
                }
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
                reviewer_agent_id: review.reviewer_agent_id,
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

fn progress_runtime(progress: &AgentProgressCheckpoint) -> StudioAgentProgressRuntime {
    StudioAgentProgressRuntime {
        stage: progress_stage_label(progress.stage).to_string(),
        summary: progress.summary.clone(),
        next_step: progress.next_step.clone(),
        revision: progress.revision,
        updated_at: progress.updated_at,
    }
}

fn summary_age_seconds(
    snapshot: Option<&AgentSnapshot>,
    fallback_updated_at: i64,
    now: i64,
) -> u64 {
    let updated_at = snapshot.map_or(fallback_updated_at, |snapshot| {
        snapshot
            .progress
            .as_ref()
            .map_or(snapshot.updated_at, |progress| progress.updated_at)
    });
    u64::try_from(now.saturating_sub(updated_at).max(0)).unwrap_or_default()
}

const fn progress_stage_label(stage: AgentProgressStage) -> &'static str {
    match stage {
        AgentProgressStage::Exploring => "exploring",
        AgentProgressStage::Implementing => "implementing",
        AgentProgressStage::Verifying => "verifying",
        AgentProgressStage::Blocked => "blocked",
        AgentProgressStage::ReadyForCompletion => "readyForCompletion",
        AgentProgressStage::ReadyForReview => "readyForReview",
    }
}

const fn lifecycle_label(lifecycle: AgentLifecycleState) -> &'static str {
    match lifecycle {
        AgentLifecycleState::Active => "active",
        AgentLifecycleState::Closing => "closing",
        AgentLifecycleState::Closed => "closed",
        AgentLifecycleState::Faulted => "faulted",
    }
}

const fn activity_label(activity: AgentActivityState) -> &'static str {
    match activity {
        AgentActivityState::Idle => "idle",
        AgentActivityState::Queued => "queued",
        AgentActivityState::Running => "running",
        AgentActivityState::WaitingTool => "waitingTool",
        AgentActivityState::WaitingInteraction => "waitingInteraction",
        AgentActivityState::Cancelling => "cancelling",
    }
}

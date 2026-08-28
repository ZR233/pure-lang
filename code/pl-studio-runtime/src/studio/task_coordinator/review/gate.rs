use anyhow::{Context, Result, bail};

use super::super::{
    MergeRecord, ReviewRoundRecord, ReviewScope, ReviewVerdict, TaskRun, WorkCompletionKind,
    WorkCompletionRecord, WorkCompletionStatus, WorkUnit, WorkUnitCompletionOutcome,
    WorkUnitStateKind, current_work_units,
};
use crate::StudioIntegratedReviewGate;

pub(crate) async fn integrated_review_gate(
    run: &TaskRun,
    work_units: &[WorkUnit],
    completions: &[WorkCompletionRecord],
    merges: &[MergeRecord],
    reviews: &[ReviewRoundRecord],
) -> StudioIntegratedReviewGate {
    integrated_review_gate_now(run, work_units, completions, merges, reviews)
}

pub(crate) fn integrated_review_gate_now(
    _run: &TaskRun,
    work_units: &[WorkUnit],
    completions: &[WorkCompletionRecord],
    merges: &[MergeRecord],
    reviews: &[ReviewRoundRecord],
) -> StudioIntegratedReviewGate {
    let integrated_reviews = reviews
        .iter()
        .filter(|review| review.scope == ReviewScope::Integrated)
        .collect::<Vec<_>>();
    if let Some(latest) = integrated_reviews.iter().max_by_key(|review| review.round) {
        let latest_merge_head = merges
            .iter()
            .max_by_key(|merge| (merge.created_at, &merge.id))
            .map(|merge| merge.resulting_head.as_str());
        if latest.verdict() == ReviewVerdict::Pass
            && latest_merge_head == Some(latest.reviewed_head.as_str())
        {
            return StudioIntegratedReviewGate::SatisfiedByReview {
                review_round_id: latest.id.clone(),
                reviewed_head: latest.reviewed_head.clone(),
            };
        }
        return required(format!(
            "已有综合审查轮次 {}，但它未通过最新的持久化合并声明",
            latest.id
        ));
    }

    if merges.is_empty() {
        return if current_work_units(work_units).iter().all(|work_unit| {
            matches!(
                work_unit.completion_outcome(),
                Some(WorkUnitCompletionOutcome::NoDelivery { .. })
            )
        }) {
            StudioIntegratedReviewGate::NotRequiredNoDelivery
        } else {
            required("仍有工作单未完成无需交付结算")
        };
    }

    let candidate = match single_executor_candidate(work_units, completions, merges, reviews) {
        Ok(candidate) => candidate,
        Err(error) => return required(error.to_string()),
    };
    if let Err(error) = prove_single_executor_equivalence(reviews, &candidate) {
        return required(format!("无法复用交付审查：{error}"));
    }
    StudioIntegratedReviewGate::NotRequiredSingleExecutorEquivalent {
        work_unit_id: candidate.work_unit.id.clone(),
        completion_revision: candidate.completion.revision,
        merge_record_id: candidate.merge.id.clone(),
    }
}

fn prove_single_executor_equivalence(
    reviews: &[ReviewRoundRecord],
    candidate: &SingleExecutorCandidate<'_>,
) -> Result<()> {
    if candidate.completion.base_commit != candidate.merge.expected_previous_head {
        bail!("获准 completion 的基础提交与合并前任务提交不同")
    }
    let delivery_head = candidate
        .completion
        .head_commit()
        .context("获准 delivery completion 缺少提交")?;
    if delivery_head != candidate.merge.delivery_head {
        bail!("MergeRecord 的交付提交不是获准 completion 提交")
    }

    let delivery_review = reviews
        .iter()
        .find(|review| {
            review.scope == ReviewScope::Delivery
                && review.verdict() == ReviewVerdict::Pass
                && review.work_unit_id.as_deref() == Some(candidate.work_unit.id.as_str())
                && review.completion_id.as_deref() == Some(candidate.completion.id.as_str())
                && review.completion_revision == Some(candidate.completion.revision)
                && review.reviewed_head == delivery_head
        })
        .context("获准 completion 没有对应的通过交付审查")?;
    if !delivery_review.kind().is_terminal() {
        bail!("交付审查者尚未结束")
    }
    if !candidate.work_unit.kind().is_terminal() {
        bail!("执行者尚未结束")
    }
    if reviews.iter().any(|review| review.kind().is_active()) {
        bail!("仍有任务审查者未结束")
    }

    Ok(())
}

struct SingleExecutorCandidate<'a> {
    work_unit: &'a WorkUnit,
    completion: &'a WorkCompletionRecord,
    merge: &'a MergeRecord,
}

fn single_executor_candidate<'a>(
    work_units: &'a [WorkUnit],
    completions: &'a [WorkCompletionRecord],
    merges: &'a [MergeRecord],
    reviews: &[ReviewRoundRecord],
) -> Result<SingleExecutorCandidate<'a>> {
    let [work_unit] = work_units else {
        bail!("整个任务生命周期不是恰好一个工作单")
    };
    let executor_id = work_unit
        .executor_thread_id
        .as_deref()
        .context("唯一工作单缺少执行者身份")?;
    if work_unit.kind() != WorkUnitStateKind::Completed
        || !matches!(
            work_unit.completion_outcome(),
            Some(WorkUnitCompletionOutcome::Merged { .. })
        )
    {
        bail!("唯一工作单尚未完成合并")
    }
    let [merge] = merges else {
        bail!("整个任务生命周期不是恰好一个合并记录")
    };
    if merge.work_unit_id != work_unit.id || merge.executor_agent_id != executor_id {
        bail!("合并记录不属于唯一执行者工作单")
    }
    let approved = completions
        .iter()
        .filter(|completion| completion.status() == WorkCompletionStatus::Approved)
        .collect::<Vec<_>>();
    let [completion] = approved.as_slice() else {
        bail!("任务不是恰好一个获准 completion")
    };
    if completion.kind() != WorkCompletionKind::Delivery
        || completion.work_unit_id != work_unit.id
        || completion.executor_agent_id != executor_id
        || merge.completion_id != completion.id
        || merge.completion_revision != completion.revision
    {
        bail!("唯一获准 delivery 与合并记录不一致")
    }
    let passing_delivery_reviews = reviews
        .iter()
        .filter(|review| {
            review.scope == ReviewScope::Delivery && review.verdict() == ReviewVerdict::Pass
        })
        .count();
    if passing_delivery_reviews != 1 {
        bail!("任务不是恰好一个通过的交付审查")
    }
    Ok(SingleExecutorCandidate {
        work_unit,
        completion,
        merge,
    })
}

fn required(reason: impl Into<String>) -> StudioIntegratedReviewGate {
    StudioIntegratedReviewGate::Required {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::task_coordinator::{
        MergeCleanupState, MergeMethod, ReviewPassedOutcome, ReviewRoundCommand, ReviewRoundState,
        TaskContext, TaskRunState, WorkCompletionCommand, WorkCompletionContent,
        WorkCompletionState, WorkUnitCommand, WorkUnitContext, WorkUnitState,
    };

    #[test]
    fn single_executor_review_reuse_allows_distinct_resulting_head() {
        let run = TaskRun {
            context: TaskContext {
                id: "task-1".to_string(),
                project_id: "project-1".to_string(),
                root_thread_id: "thread-root".to_string(),
                request: "request".to_string(),
                plan: None,
                workspace_root: "workspace".to_string(),
            },
            state: TaskRunState::new(),
            revision: 0,
            created_at: 1,
            updated_at: 1,
        };
        let work_unit = completed_work_unit();
        let completion = approved_completion();
        let merge = MergeRecord {
            id: "merge-1".to_string(),
            task_run_id: "task-1".to_string(),
            work_unit_id: "work-1".to_string(),
            completion_id: "completion-1".to_string(),
            completion_revision: 1,
            executor_agent_id: "executor-1".to_string(),
            expected_previous_head: "base".to_string(),
            resulting_head: "cherry-picked-head".to_string(),
            delivery_head: "delivery-head".to_string(),
            method: MergeMethod::CherryPick,
            summary: "merged".to_string(),
            cleanup: MergeCleanupState::pending(),
            revision: 0,
            created_at: 1,
            updated_at: 1,
        };
        let review = passing_delivery_review();

        assert_eq!(
            integrated_review_gate_now(&run, &[work_unit], &[completion], &[merge], &[review],),
            StudioIntegratedReviewGate::NotRequiredSingleExecutorEquivalent {
                work_unit_id: "work-1".to_string(),
                completion_revision: 1,
                merge_record_id: "merge-1".to_string(),
            }
        );
    }

    fn completed_work_unit() -> WorkUnit {
        let state = next_work_unit(WorkUnitState::pending(), WorkUnitCommand::Activate);
        let state = next_work_unit(
            state,
            WorkUnitCommand::SubmitCompletion {
                completion_id: "completion-1".to_string(),
                completion_revision: 1,
                verification_summary: "verified".to_string(),
            },
        );
        let state = next_work_unit(
            state,
            WorkUnitCommand::BeginReview {
                review_round_id: "review-1".to_string(),
            },
        );
        let state = next_work_unit(
            state,
            WorkUnitCommand::PassReview {
                review_round_id: "review-1".to_string(),
                outcome: ReviewPassedOutcome::Delivery,
            },
        );
        let state = next_work_unit(
            state,
            WorkUnitCommand::CompleteMerge {
                merge_record_id: "merge-1".to_string(),
            },
        );
        WorkUnit {
            context: WorkUnitContext {
                id: "work-1".to_string(),
                task_run_id: "task-1".to_string(),
                title: "work".to_string(),
                scope_hints: Vec::new(),
                blueprint: None,
                base_commit: "base".to_string(),
                worktree_path: "worktree".to_string(),
                branch: "branch".to_string(),
                attempt: 1,
                supersedes_work_unit_id: None,
                executor_thread_id: Some("executor-1".to_string()),
                requested_by_call_id: "spawn-call".to_string(),
            },
            state,
            revision: 1,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn next_work_unit(state: WorkUnitState, command: WorkUnitCommand) -> WorkUnitState {
        state.decide("work-1", command).unwrap().next_state()
    }

    fn approved_completion() -> WorkCompletionRecord {
        let state = WorkCompletionState::ready_for_review()
            .decide(
                "completion-1",
                WorkCompletionCommand::Approve {
                    review_round_id: "review-1".to_string(),
                    decided_at: 1,
                },
            )
            .unwrap()
            .next_state();
        WorkCompletionRecord {
            id: "completion-1".to_string(),
            task_run_id: "task-1".to_string(),
            work_unit_id: "work-1".to_string(),
            executor_agent_id: "executor-1".to_string(),
            revision: 1,
            content: WorkCompletionContent::delivery(
                "delivery-head".to_string(),
                vec!["README.md".to_string()],
            )
            .unwrap(),
            state,
            state_revision: 1,
            base_commit: "base".to_string(),
            verification_summary: "verified".to_string(),
            worktree_path: "worktree".to_string(),
            branch: "branch".to_string(),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn passing_delivery_review() -> ReviewRoundRecord {
        let reviewer_thread_id = "reviewer-1".to_string();
        let state = ReviewRoundState::pending_dispatch()
            .decide(
                "review-1",
                ReviewRoundCommand::Dispatch {
                    reviewer_thread_id: reviewer_thread_id.clone(),
                },
            )
            .unwrap()
            .next_state();
        let state = state
            .decide(
                "review-1",
                ReviewRoundCommand::Start {
                    reviewer_thread_id: reviewer_thread_id.clone(),
                },
            )
            .unwrap()
            .next_state();
        let state = state
            .decide(
                "review-1",
                ReviewRoundCommand::Pass {
                    reviewer_thread_id,
                    summary: "pass".to_string(),
                },
            )
            .unwrap()
            .next_state();
        ReviewRoundRecord {
            id: "review-1".to_string(),
            task_run_id: "task-1".to_string(),
            round: 1,
            scope: ReviewScope::Delivery,
            work_unit_id: Some("work-1".to_string()),
            completion_id: Some("completion-1".to_string()),
            completion_revision: Some(1),
            reviewed_head: "delivery-head".to_string(),
            requested_by_call_id: "review-call".to_string(),
            state,
            design_references: Vec::new(),
            findings: Vec::new(),
            file_reviews: None,
            revision: 1,
            created_at: 1,
            updated_at: 1,
        }
    }
}

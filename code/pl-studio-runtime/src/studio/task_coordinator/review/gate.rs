use anyhow::{Context, Result, bail};

use super::super::git::{
    GitDiffSelection, changed_files_between_selected, ensure_no_git_operation, inspect_repository,
    is_ancestor, resolve_tree_oid,
};
use super::super::{
    MergeRecord, ReviewRoundRecord, ReviewScope, ReviewVerdict, TaskRunRecord,
    ThreadExecutionStatus, WorkCompletionKind, WorkCompletionRecord, WorkCompletionStatus,
    WorkUnitRecord, WorkUnitStatus,
};
use crate::StudioIntegratedReviewGate;

pub(crate) async fn integrated_review_gate(
    run: &TaskRunRecord,
    work_units: &[WorkUnitRecord],
    completions: &[WorkCompletionRecord],
    merges: &[MergeRecord],
    reviews: &[ReviewRoundRecord],
) -> StudioIntegratedReviewGate {
    let integrated_reviews = reviews
        .iter()
        .filter(|review| review.scope == ReviewScope::Integrated)
        .collect::<Vec<_>>();
    if let Some(latest) = integrated_reviews.iter().max_by_key(|review| review.round) {
        if latest.verdict == ReviewVerdict::Pass && latest.reviewed_head == run.expected_head {
            return StudioIntegratedReviewGate::SatisfiedByReview {
                review_round_id: latest.id.clone(),
                reviewed_head: latest.reviewed_head.clone(),
            };
        }
        return required(format!(
            "已有综合审查轮次 {}，但它未通过当前任务提交 {}",
            latest.id, run.expected_head
        ));
    }

    if merges.is_empty() {
        return if work_units
            .iter()
            .all(|work_unit| work_unit.status == WorkUnitStatus::NoDelivery)
        {
            StudioIntegratedReviewGate::NotRequiredNoDelivery
        } else {
            required("仍有工作单未完成无需交付结算")
        };
    }

    let candidate = match single_executor_candidate(work_units, completions, merges, reviews) {
        Ok(candidate) => candidate,
        Err(error) => return required(error.to_string()),
    };
    if let Err(error) = prove_single_executor_equivalence(run, reviews, &candidate).await {
        return required(format!("无法复用交付审查：{error}"));
    }
    StudioIntegratedReviewGate::NotRequiredSingleExecutorEquivalent {
        work_unit_id: candidate.work_unit.id.clone(),
        completion_revision: candidate.completion.revision,
        merge_record_id: candidate.merge.id.clone(),
    }
}

async fn prove_single_executor_equivalence(
    run: &TaskRunRecord,
    reviews: &[ReviewRoundRecord],
    candidate: &SingleExecutorCandidate<'_>,
) -> Result<()> {
    if run.design_commit.as_deref() != Some(run.expected_head.as_str()) {
        bail!("最终设计提交不是当前任务提交")
    }
    if candidate.completion.base_commit != candidate.merge.expected_previous_head {
        bail!("获准 completion 的基础提交与合并前任务提交不同")
    }
    let delivery_head = candidate
        .completion
        .head_commit
        .as_deref()
        .context("获准 delivery completion 缺少提交")?;
    if delivery_head != candidate.merge.delivery_head {
        bail!("MergeRecord 的交付提交不是获准 completion 提交")
    }

    let delivery_review = reviews
        .iter()
        .find(|review| {
            review.scope == ReviewScope::Delivery
                && review.verdict == ReviewVerdict::Pass
                && review.work_unit_id.as_deref() == Some(candidate.work_unit.id.as_str())
                && review.completion_id.as_deref() == Some(candidate.completion.id.as_str())
                && review.completion_revision == Some(candidate.completion.revision)
                && review.reviewed_head == delivery_head
        })
        .context("获准 completion 没有对应的通过交付审查")?;
    if !is_terminal(reviewed_status(delivery_review)) {
        bail!("交付审查者尚未结束")
    }
    if !is_terminal(candidate.work_unit.execution_status) {
        bail!("执行者尚未结束")
    }
    if reviews
        .iter()
        .any(|review| !is_terminal(review.reviewer_status))
    {
        bail!("仍有任务审查者未结束")
    }

    super::validate_review_repository(run).await?;
    ensure_no_git_operation(&run.workspace_root).await?;
    let repository = inspect_repository(&run.workspace_root, true).await?;
    if repository.head != run.expected_head {
        bail!("主工作区提交已漂移")
    }

    let delivery_tree = resolve_tree_oid(&run.workspace_root, delivery_head).await?;
    let merged_tree =
        resolve_tree_oid(&run.workspace_root, &candidate.merge.resulting_head).await?;
    if delivery_tree != merged_tree {
        bail!("合并结果的完整版本树与获准交付不同")
    }
    if !is_ancestor(
        &run.workspace_root,
        &candidate.merge.resulting_head,
        &run.expected_head,
    )
    .await?
    {
        bail!("当前任务提交不继承已记录的合并结果")
    }
    let implementation_changes = changed_files_between_selected(
        &run.workspace_root,
        &candidate.merge.resulting_head,
        &run.expected_head,
        GitDiffSelection::ExcludeDesign,
    )
    .await?;
    if !implementation_changes.is_empty() {
        bail!(
            "合并后存在 design/** 之外的额外修改：{}",
            implementation_changes.join(", ")
        )
    }
    Ok(())
}

struct SingleExecutorCandidate<'a> {
    work_unit: &'a WorkUnitRecord,
    completion: &'a WorkCompletionRecord,
    merge: &'a MergeRecord,
}

fn single_executor_candidate<'a>(
    work_units: &'a [WorkUnitRecord],
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
    if work_unit.status != WorkUnitStatus::Merged {
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
        .filter(|completion| completion.status == WorkCompletionStatus::Approved)
        .collect::<Vec<_>>();
    let [completion] = approved.as_slice() else {
        bail!("任务不是恰好一个获准 completion")
    };
    if completion.kind != WorkCompletionKind::Delivery
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
            review.scope == ReviewScope::Delivery && review.verdict == ReviewVerdict::Pass
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

fn reviewed_status(review: &ReviewRoundRecord) -> ThreadExecutionStatus {
    review.reviewer_status
}

fn is_terminal(status: ThreadExecutionStatus) -> bool {
    !matches!(
        status,
        ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
    )
}

fn required(reason: impl Into<String>) -> StudioIntegratedReviewGate {
    StudioIntegratedReviewGate::Required {
        reason: reason.into(),
    }
}

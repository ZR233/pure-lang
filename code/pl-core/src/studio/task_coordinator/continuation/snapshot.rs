use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::studio::store::StudioStore;

use super::super::{
    AgentOutcomeRecord, BranchLeaseRecord, MergeRecord, ReviewRoundRecord, TaskRunRecord,
    WorkUnitRecord,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskContinuationSnapshot {
    pub(crate) run: TaskRunRecord,
    pub(crate) branch_lease: BranchLeaseRecord,
    pub(crate) work_units: Vec<WorkUnitRecord>,
    pub(crate) agent_outcomes: Vec<AgentOutcomeRecord>,
    pub(crate) merge_records: Vec<MergeRecord>,
    pub(crate) review_rounds: Vec<ReviewRoundRecord>,
}

impl TaskContinuationSnapshot {
    pub(crate) fn render_prompt(&self) -> Result<String> {
        let snapshot = serde_json::to_string_pretty(self)?;
        Ok(format!(
            "这是一次 Task planner continuation（续跑），不是新任务。\n\
             请检查当前持久化事实，并采取下一项允许的 coordinator action。\n\
             不要使用过期的内存状态，也不要无限等待代理；代理终态已包含在下方快照中。\n\n\
             <taskContinuationSnapshot>\n{snapshot}\n</taskContinuationSnapshot>"
        ))
    }
}

impl StudioStore {
    pub(crate) async fn load_task_continuation_snapshot(
        &self,
        task_run_id: &str,
    ) -> Result<TaskContinuationSnapshot> {
        let run = self
            .read_task_run(task_run_id)
            .await?
            .context("task run not found")?;
        let branch_lease = self
            .read_branch_lease(task_run_id)
            .await?
            .context("task branch lease not found")?;
        let work_units = self.list_work_units(task_run_id).await?;
        let agent_outcomes = self.list_agent_outcomes(task_run_id).await?;
        let merge_records = self.list_merge_records(task_run_id).await?;
        let review_rounds = self.list_review_rounds(task_run_id).await?;

        ensure!(run.id == task_run_id, "task continuation run mismatch");
        ensure!(
            branch_lease.task_run_id == task_run_id,
            "task continuation branch lease mismatch"
        );
        ensure_exact_children(task_run_id, &work_units, |record| &record.task_run_id)?;
        ensure_exact_children(task_run_id, &agent_outcomes, |record| &record.task_run_id)?;
        ensure_exact_children(task_run_id, &merge_records, |record| &record.task_run_id)?;
        ensure_exact_children(task_run_id, &review_rounds, |record| &record.task_run_id)?;

        Ok(TaskContinuationSnapshot {
            run,
            branch_lease,
            work_units,
            agent_outcomes,
            merge_records,
            review_rounds,
        })
    }
}

fn ensure_exact_children<T>(
    task_run_id: &str,
    records: &[T],
    record_task_run_id: impl Fn(&T) -> &String,
) -> Result<()> {
    ensure!(
        records
            .iter()
            .all(|record| record_task_run_id(record) == task_run_id),
        "task continuation child record mismatch"
    );
    Ok(())
}

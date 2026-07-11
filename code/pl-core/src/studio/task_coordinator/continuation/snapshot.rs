use anyhow::Result;
use serde::Serialize;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskContinuationResolution {
    Active(Box<TaskContinuationSnapshot>),
    Terminal(Box<TaskRunRecord>),
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

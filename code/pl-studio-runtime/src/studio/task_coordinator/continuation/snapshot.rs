use anyhow::Result;
use serde::Serialize;

use super::super::{
    AgentOutcomeRecord, AgentOutcomeStatus, BranchLeaseRecord, MergeRecord, ReviewRoundRecord,
    TaskRunRecord, WorkUnitRecord,
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
        let waiting_delivery_agents = self
            .agent_outcomes
            .iter()
            .filter(|outcome| {
                outcome.role == "executor"
                    && outcome.status == AgentOutcomeStatus::WaitingForDelivery
                    && outcome.delivery.is_none()
            })
            .map(|outcome| outcome.agent_id.as_str())
            .collect::<Vec<_>>();
        let delivery_guidance = if waiting_delivery_agents.is_empty() {
            String::new()
        } else {
            format!(
                "检测到 executor 已结束但尚未交付：{}。\n\
                 coordinator 会向对应 agent 自动投递最多一次受控 recovery；不要再调用 \
                 send_input 或 close_agent，也不要轮询状态。合同终结后订阅机制会再次提交 \
                 durable 结果。\n",
                waiting_delivery_agents.join(", ")
            )
        };
        let stop_guidance = if self.run.stop_requested {
            let origin = self
                .run
                .stop_requested_origin
                .map_or("未知来源", super::super::TaskStopOrigin::display_label);
            let reason = self
                .run
                .stop_requested_reason
                .as_deref()
                .unwrap_or("未记录原因");
            format!(
                "本任务已由{origin}发起停止（generation {}，原因：{reason}）。不要继续分配、\
                 审查或合并新工作；progress 或 inactivity diagnostic 不能改变停止来源。\
                 等待现有 delivery 合同终结后，仅允许受控 delivery recovery 或完成停止收束。\n",
                self.run.task_generation
            )
        } else {
            String::new()
        };
        Ok(format!(
            "这是一次 Task planner continuation（续跑），不是新任务。\n\
             请检查当前持久化事实，并采取下一项允许的 coordinator action。\n\
             不要使用过期的内存状态，也不要无限等待代理；代理终态已包含在下方快照中。\n\n\
             {stop_guidance}{delivery_guidance}\n\
             <taskContinuationSnapshot>\n{snapshot}\n</taskContinuationSnapshot>"
        ))
    }
}

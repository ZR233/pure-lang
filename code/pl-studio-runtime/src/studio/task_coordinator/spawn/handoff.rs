use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::{TaskRunRecord, WorkUnitRecord};

pub(crate) const TASK_EXECUTOR_HANDOFF_SECTION_ID: &str = "studio.task_executor_handoff";
const TASK_EXECUTOR_HANDOFF_VERSION: u32 = 1;

/// Planner 可以随 executor allocation 一起提交的结构化依赖。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorDependencyV1 {
    pub(crate) kind: String,
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

/// 已完成探索的稳定定位证据；正文留在原文件或 child transcript。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorEvidenceV1 {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) line: Option<u32>,
    #[serde(default)]
    pub(crate) symbol: Option<String>,
    #[serde(default)]
    pub(crate) content_hash: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

/// Planner 在 allocation 前核对的单条项目验证命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorVerificationCommandV1 {
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) purpose: String,
}

/// Fresh executor 不依赖 planner transcript 也能恢复的验证契约。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorVerificationContractV1 {
    pub(crate) commands: Vec<TaskExecutorVerificationCommandV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorDeliveryContractV1 {
    pub(crate) completion_tool: String,
    pub(crate) require_clean_worktree: bool,
    pub(crate) require_commit: bool,
    pub(crate) require_verification_summary: bool,
}

/// Task executor 的 durable、版本化交接事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorHandoffV1 {
    pub(crate) version: u32,
    pub(crate) task_run_id: String,
    pub(crate) work_unit_id: String,
    pub(crate) executor_agent_id: String,
    pub(crate) parent_thread_id: String,
    pub(crate) requesting_call_id: String,
    pub(crate) task_name: String,
    pub(crate) assignment: String,
    pub(crate) confirmed_task_plan: String,
    pub(crate) base_commit: String,
    pub(crate) design_commit: Option<String>,
    pub(crate) expected_head_at_spawn: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) scope_hints: Vec<String>,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) dependencies: Vec<TaskExecutorDependencyV1>,
    pub(crate) evidence: Vec<TaskExecutorEvidenceV1>,
    #[serde(default)]
    pub(crate) verification: TaskExecutorVerificationContractV1,
    pub(crate) delivery: TaskExecutorDeliveryContractV1,
}

/// Durable owner 之外由 planner 提供的 executor 交接内容。
pub(crate) struct TaskExecutorHandoffInput {
    pub(crate) parent_thread_id: String,
    pub(crate) assignment: String,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) dependencies: Vec<TaskExecutorDependencyV1>,
    pub(crate) evidence: Vec<TaskExecutorEvidenceV1>,
    pub(crate) verification_commands: Vec<TaskExecutorVerificationCommandV1>,
}

impl TaskExecutorHandoffV1 {
    pub(crate) fn new(
        run: &TaskRunRecord,
        work_unit: &WorkUnitRecord,
        input: TaskExecutorHandoffInput,
    ) -> Self {
        Self {
            version: TASK_EXECUTOR_HANDOFF_VERSION,
            task_run_id: run.id.clone(),
            work_unit_id: work_unit.id.clone(),
            executor_agent_id: work_unit.executor_thread_id.clone().unwrap_or_default(),
            parent_thread_id: input.parent_thread_id,
            requesting_call_id: work_unit.requested_by_call_id.clone(),
            task_name: work_unit.title.clone(),
            assignment: input.assignment,
            confirmed_task_plan: run.plan.clone(),
            base_commit: work_unit.base_commit.clone(),
            design_commit: run.design_commit.clone(),
            expected_head_at_spawn: run.expected_head.clone(),
            worktree_path: work_unit.worktree_path.clone(),
            branch: work_unit.branch.clone(),
            scope_hints: work_unit.scope_hints.clone(),
            acceptance_criteria: input.acceptance_criteria,
            dependencies: input.dependencies,
            evidence: input.evidence,
            verification: TaskExecutorVerificationContractV1 {
                commands: input.verification_commands,
            },
            delivery: TaskExecutorDeliveryContractV1 {
                completion_tool: "report_completion".to_string(),
                require_clean_worktree: true,
                require_commit: true,
                require_verification_summary: true,
            },
        }
    }

    pub(crate) fn to_context_section(&self) -> Result<pl_core::PinnedContextSection> {
        let content = serde_json::to_string_pretty(self)
            .context("failed to serialize Task executor handoff")?;
        pl_core::context_section(
            TASK_EXECUTOR_HANDOFF_SECTION_ID,
            u64::from(self.version),
            "Task executor handoff",
            content,
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    pub(crate) fn from_context_section(section: &pl_core::PinnedContextSection) -> Result<Self> {
        if section.id.as_str() != TASK_EXECUTOR_HANDOFF_SECTION_ID {
            bail!("unexpected Task executor handoff section id")
        }
        let handoff: Self = serde_json::from_str(&section.content)
            .context("invalid Task executor handoff payload")?;
        if handoff.version != TASK_EXECUTOR_HANDOFF_VERSION {
            bail!(
                "unsupported Task executor handoff version {}",
                handoff.version
            )
        }
        Ok(handoff)
    }

    pub(crate) fn validate_owner(
        &self,
        run: &TaskRunRecord,
        work_unit: &WorkUnitRecord,
        executor_agent_id: &str,
    ) -> Result<()> {
        if self.task_run_id != run.id
            || self.work_unit_id != work_unit.id
            || self.executor_agent_id != executor_agent_id
            || self.requesting_call_id != work_unit.requested_by_call_id
            || self.parent_thread_id != run.root_thread_id
            || self.base_commit != work_unit.base_commit
            || self.expected_head_at_spawn != work_unit.base_commit
            || self.worktree_path != work_unit.worktree_path
            || self.branch != work_unit.branch
        {
            bail!("Task executor handoff does not match its durable owner")
        }
        Ok(())
    }
}

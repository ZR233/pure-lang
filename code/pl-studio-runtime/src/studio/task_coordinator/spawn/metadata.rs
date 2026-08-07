use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{TaskExecutorDependencyV1, TaskExecutorEvidenceV1, TaskExecutorVerificationCommandV1};

/// Studio Task harness 创建 child agent 时写入 lifecycle 的可信意图类型。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum StudioSpawnKind {
    TaskExecutor,
    TaskReviewer,
}

/// Task executor spawn 所需的结构化产品输入。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StudioTaskExecutorIntent {
    pub(crate) thread_id: String,
    pub(crate) task_name: String,
    pub(crate) scope_hints: Vec<String>,
    pub(crate) requesting_tool_call_id: String,
    pub(crate) subagent_constraint: String,
    pub(crate) assignment: String,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) dependencies: Vec<TaskExecutorDependencyV1>,
    pub(crate) evidence: Vec<TaskExecutorEvidenceV1>,
    pub(crate) verification_commands: Vec<TaskExecutorVerificationCommandV1>,
}

/// Studio lifecycle 从 framework spawn metadata 一次解析出的产品输入。
///
/// 通用 explorer 允许不带 `spawnKind`；executor 和 reviewer 必须携带由各自
/// 产品 harness 构造的 kind，不能从模型自由 metadata 推断。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StudioSpawnIntent {
    #[serde(default)]
    pub(crate) spawn_kind: Option<StudioSpawnKind>,
    #[serde(default)]
    pub(crate) studio_thread_id: Option<String>,
    #[serde(default)]
    pub(crate) task_name: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) scope_hints: Vec<String>,
    #[serde(default)]
    pub(crate) requesting_tool_call_id: Option<String>,
    #[serde(default)]
    pub(crate) review_round_id: Option<String>,
    #[serde(default)]
    pub(crate) subagent_constraint: Option<String>,
    #[serde(default)]
    pub(crate) assignment: Option<String>,
    #[serde(default)]
    pub(crate) acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub(crate) dependencies: Vec<TaskExecutorDependencyV1>,
    #[serde(default)]
    pub(crate) evidence: Vec<TaskExecutorEvidenceV1>,
    #[serde(default)]
    pub(crate) verification_commands: Vec<TaskExecutorVerificationCommandV1>,
}

impl StudioSpawnIntent {
    pub(crate) fn task_executor(input: StudioTaskExecutorIntent) -> Self {
        Self {
            spawn_kind: Some(StudioSpawnKind::TaskExecutor),
            studio_thread_id: Some(input.thread_id),
            task_name: Some(input.task_name),
            scope_hints: input.scope_hints,
            requesting_tool_call_id: Some(input.requesting_tool_call_id),
            subagent_constraint: Some(input.subagent_constraint),
            assignment: Some(input.assignment),
            acceptance_criteria: input.acceptance_criteria,
            dependencies: input.dependencies,
            evidence: input.evidence,
            verification_commands: input.verification_commands,
            ..Self::default()
        }
    }

    pub(crate) fn task_reviewer(
        thread_id: impl Into<String>,
        task_name: impl Into<String>,
        requesting_tool_call_id: impl Into<String>,
        review_round_id: impl Into<String>,
        subagent_constraint: impl Into<String>,
    ) -> Self {
        Self {
            spawn_kind: Some(StudioSpawnKind::TaskReviewer),
            studio_thread_id: Some(thread_id.into()),
            task_name: Some(task_name.into()),
            requesting_tool_call_id: Some(requesting_tool_call_id.into()),
            review_round_id: Some(review_round_id.into()),
            subagent_constraint: Some(subagent_constraint.into()),
            ..Self::default()
        }
    }

    pub(crate) fn parse(value: serde_json::Value) -> Result<Self> {
        if value.is_null() {
            return Ok(Self::default());
        }
        serde_json::from_value(value).context("invalid Studio spawn intent")
    }

    pub(crate) fn validate_role(&self, role: &str) -> Result<()> {
        match role {
            "executor" if self.spawn_kind != Some(StudioSpawnKind::TaskExecutor) => {
                bail!("Task executor must be created with task_spawn_executor")
            }
            "reviewer" if self.spawn_kind != Some(StudioSpawnKind::TaskReviewer) => {
                bail!("Task reviewer must be created by a Task review request")
            }
            "explorer" if self.spawn_kind.is_some() => {
                bail!("explorer must be created with spawn_agent")
            }
            _ => {}
        }
        match role {
            "executor" | "reviewer" => {
                require_non_empty(self.studio_thread_id.as_deref(), "Studio Thread id")?;
                require_non_empty(self.task_name.as_deref(), "task name")?;
                require_non_empty(
                    self.requesting_tool_call_id.as_deref(),
                    "requesting tool call id",
                )?;
                require_non_empty(self.subagent_constraint.as_deref(), "subagent constraint")?;
            }
            _ => {}
        }
        if role == "executor" {
            require_non_empty(self.assignment.as_deref(), "executor assignment")?;
            if self.verification_commands.is_empty() {
                bail!("Task executor must have a verification contract")
            }
        }
        if role != "executor" && !self.scope_hints.is_empty() {
            bail!("{role} must not declare scopeHints");
        }
        if role == "reviewer" {
            require_non_empty(self.review_round_id.as_deref(), "review round id")?;
        }
        Ok(())
    }

    pub(crate) fn task_name(&self, role: &str) -> String {
        self.task_name
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or(role)
            .to_string()
    }

    pub(crate) fn requesting_tool_call_id(&self) -> String {
        self.requesting_tool_call_id
            .clone()
            .unwrap_or_else(|| "spawn_agent".to_string())
    }
}

fn require_non_empty(value: Option<&str>, field: &str) -> Result<()> {
    if value.is_none_or(|value| value.trim().is_empty()) {
        bail!("Task spawn intent has no {field}");
    }
    Ok(())
}

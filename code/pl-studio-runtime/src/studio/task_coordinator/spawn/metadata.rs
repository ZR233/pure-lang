use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Studio Task harness 创建 child agent 时写入 lifecycle 的可信意图类型。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum StudioSpawnKind {
    TaskExecutor,
    TaskReviewer,
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
}

impl StudioSpawnIntent {
    pub(crate) fn task_executor(
        thread_id: impl Into<String>,
        task_name: impl Into<String>,
        scope_hints: Vec<String>,
        requesting_tool_call_id: impl Into<String>,
        subagent_constraint: impl Into<String>,
    ) -> Self {
        Self {
            spawn_kind: Some(StudioSpawnKind::TaskExecutor),
            studio_thread_id: Some(thread_id.into()),
            task_name: Some(task_name.into()),
            scope_hints,
            requesting_tool_call_id: Some(requesting_tool_call_id.into()),
            subagent_constraint: Some(subagent_constraint.into()),
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

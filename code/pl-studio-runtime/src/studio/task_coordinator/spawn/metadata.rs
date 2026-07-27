use std::path::PathBuf;

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
    pub(crate) studio_session_id: Option<String>,
    #[serde(default)]
    pub(crate) task_name: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) owned_paths: Vec<String>,
    #[serde(default)]
    pub(crate) requesting_tool_call_id: Option<String>,
    #[serde(default)]
    pub(crate) workspace_root: Option<PathBuf>,
    #[serde(default)]
    pub(crate) subagent_constraint: Option<String>,
}

impl StudioSpawnIntent {
    pub(crate) fn task_executor(
        session_id: impl Into<String>,
        task_name: impl Into<String>,
        owned_paths: Vec<String>,
        requesting_tool_call_id: impl Into<String>,
        workspace_root: PathBuf,
        subagent_constraint: impl Into<String>,
    ) -> Self {
        Self {
            spawn_kind: Some(StudioSpawnKind::TaskExecutor),
            studio_session_id: Some(session_id.into()),
            task_name: Some(task_name.into()),
            owned_paths,
            requesting_tool_call_id: Some(requesting_tool_call_id.into()),
            workspace_root: Some(workspace_root),
            subagent_constraint: Some(subagent_constraint.into()),
            ..Self::default()
        }
    }

    pub(crate) fn task_reviewer(
        session_id: impl Into<String>,
        task_name: impl Into<String>,
        requesting_tool_call_id: impl Into<String>,
        workspace_root: PathBuf,
        subagent_constraint: impl Into<String>,
    ) -> Self {
        Self {
            spawn_kind: Some(StudioSpawnKind::TaskReviewer),
            studio_session_id: Some(session_id.into()),
            task_name: Some(task_name.into()),
            requesting_tool_call_id: Some(requesting_tool_call_id.into()),
            workspace_root: Some(workspace_root),
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
                bail!("Task reviewer must be created with task_request_review")
            }
            "explorer" if self.spawn_kind.is_some() => {
                bail!("explorer must be created with spawn_agent")
            }
            _ => {}
        }
        match role {
            "executor" | "reviewer" => {
                require_non_empty(self.studio_session_id.as_deref(), "Studio session id")?;
                require_non_empty(self.task_name.as_deref(), "task name")?;
                require_non_empty(
                    self.requesting_tool_call_id.as_deref(),
                    "requesting tool call id",
                )?;
                require_non_empty(self.subagent_constraint.as_deref(), "subagent constraint")?;
                if self.workspace_root.is_none() {
                    bail!("Task {role} spawn intent has no workspace root");
                }
            }
            _ => {}
        }
        match role {
            "executor" if self.owned_paths.is_empty() => {
                bail!("Task executor ownedPaths must not be empty")
            }
            "explorer" | "reviewer" if !self.owned_paths.is_empty() => {
                bail!("{role} must not declare ownedPaths")
            }
            _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_roles_require_their_product_harness_kind() {
        let error = StudioSpawnIntent::default()
            .validate_role("executor")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Task executor must be created with task_spawn_executor"
        );
        let error = StudioSpawnIntent::default()
            .validate_role("reviewer")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Task reviewer must be created with task_request_review"
        );
    }

    #[test]
    fn task_executor_intent_carries_required_lifecycle_fields() {
        let intent = StudioSpawnIntent::task_executor(
            "session",
            "task",
            vec!["src/**".to_string()],
            "call",
            PathBuf::from("workspace"),
            "constraint",
        );

        intent.validate_role("executor").unwrap();
        assert_eq!(intent.spawn_kind, Some(StudioSpawnKind::TaskExecutor));
        assert_eq!(intent.owned_paths, vec!["src/**"]);
    }

    #[test]
    fn generic_explorer_metadata_stays_product_agnostic() {
        let intent = StudioSpawnIntent::parse(serde_json::json!({
            "name": "inspect",
            "requestingToolCallId": "call",
            "customDisplayField": true
        }))
        .unwrap();

        intent.validate_role("explorer").unwrap();
        assert_eq!(intent.task_name("explorer"), "inspect");
    }
}

use pl_protocol::{LabeledEnum, ThreadMode, UnknownLabelError};
use serde::{Deserialize, Serialize};

/// Pure Studio 的产品交互模式。
///
/// 模式只决定 Studio 选择的角色、指令、工具策略与完成方式；`pl-core`
/// 不感知这些产品语义。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StudioMode {
    #[default]
    Simple,
    Task,
}

impl StudioMode {
    pub fn instructions_for(self, role: &str, is_root: bool) -> &'static str {
        if !is_root {
            return match role {
                "explorer" => include_str!("../prompts/explorer.md"),
                "executor" => include_str!("../prompts/task_executor.md"),
                "reviewer" => include_str!("../prompts/task_reviewer.md"),
                _ => include_str!("../prompts/task_child.md"),
            };
        }
        match self {
            Self::Simple => include_str!("../prompts/simple.md"),
            Self::Task => include_str!("../prompts/task.md"),
        }
    }

    pub fn label(self) -> &'static str {
        ThreadMode::from(self).label()
    }

    /// root Thread 的角色是 mode 的派生投影：Simple 对应 executor，Task 对应 planner。
    pub const fn root_role(self) -> crate::config::StudioRole {
        match self {
            Self::Simple => crate::config::StudioRole::Executor,
            Self::Task => crate::config::StudioRole::Planner,
        }
    }

    pub fn from_label(label: &str) -> Result<Self, UnknownLabelError> {
        ThreadMode::from_label(label).map(Self::from)
    }
}

impl From<StudioMode> for ThreadMode {
    fn from(value: StudioMode) -> Self {
        match value {
            StudioMode::Simple => Self::Simple,
            StudioMode::Task => Self::Task,
        }
    }
}

impl From<ThreadMode> for StudioMode {
    fn from(value: ThreadMode) -> Self {
        match value {
            ThreadMode::Simple => Self::Simple,
            ThreadMode::Task => Self::Task,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_root_and_child_roles_receive_isolated_prompts() {
        let planner = StudioMode::Task.instructions_for("planner", true);
        assert!(planner.contains("TaskRun 只有 `planning`"));
        assert!(planner.contains("`task_transition` 提交主状态事实"));
        assert!(planner.contains("状态不限制普通读取、写入、命令或 Git 工具"));
        assert!(planner.contains("同一项目允许多个根会话的 Task 并行"));

        let explorer = StudioMode::Task.instructions_for("explorer", false);
        assert!(explorer.contains("只读探索"));
        assert!(explorer.contains("不得调用 `task_transition`"));

        let executor = StudioMode::Task.instructions_for("executor", false);
        assert!(executor.contains("以 `report_completion` 结束"));
        assert!(executor.contains("不得调用 `task_transition`"));

        let reviewer = StudioMode::Task.instructions_for("reviewer", false);
        assert!(reviewer.contains("以 `review_exit` 结束"));
        assert!(reviewer.contains("不得调用 `task_transition`"));

        let unknown = StudioMode::Task.instructions_for("custom", false);
        assert!(unknown.contains("不得调用 `task_transition`"));
        assert!(unknown.contains("不承担 root planner"));
    }

    #[test]
    fn mode_labels_round_trip_and_reject_unknown_values() {
        for mode in [StudioMode::Simple, StudioMode::Task] {
            assert_eq!(StudioMode::from_label(mode.label()), Ok(mode));
        }
        assert!(StudioMode::from_label("legacy").is_err());
    }
}

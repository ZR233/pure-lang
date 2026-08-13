/// Pure Studio 的产品交互模式。
///
/// 模式只决定 Studio 选择的角色、指令、工具策略与完成方式；`pl-core`
/// 不感知这些产品语义。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

    pub const fn label(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Task => "task",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "task" => Self::Task,
            "simple" => Self::Simple,
            _ => Self::Simple,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_root_and_child_roles_receive_isolated_prompts() {
        let planner = StudioMode::Task.instructions_for("planner", true);
        assert!(planner.contains("通过 `plan_exit` 提交可执行计划"));

        let explorer = StudioMode::Task.instructions_for("explorer", false);
        assert!(explorer.contains("只读探索"));
        assert!(explorer.contains("不得调用 `plan_exit`"));
        assert!(!explorer.contains("理解充分后通过 `plan_exit` 提交可执行计划"));

        let executor = StudioMode::Task.instructions_for("executor", false);
        assert!(executor.contains("以 `report_completion` 结束"));
        assert!(executor.contains("不得调用 `plan_exit`"));

        let reviewer = StudioMode::Task.instructions_for("reviewer", false);
        assert!(reviewer.contains("以 `review_exit` 结束"));
        assert!(reviewer.contains("不得调用 `plan_exit`"));

        let unknown = StudioMode::Task.instructions_for("custom", false);
        assert!(unknown.contains("不得调用 `plan_exit`"));
        assert!(unknown.contains("不承担 root planner"));
    }
}

use super::CompileMode;

/// 工具对运行环境产生的副作用类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolEffect {
    Read,
    WorkspaceWrite,
    Process,
    AgentControl,
    BranchControl,
    ConflictWrite,
}

impl ToolEffect {
    /// 返回内建工具的 effect；动态工具没有声明时返回 `None`。
    pub fn for_builtin_name(name: &str) -> Option<Self> {
        if name.starts_with("lsp_query_") {
            return Some(Self::Read);
        }
        match name {
            "read_file"
            | "list_files"
            | "search_files"
            | "stat_path"
            | "skills_list"
            | "skill_view"
            | "request_user_input"
            | "update_todo_list"
            | "plan_exit"
            | "list_mcp_resources"
            | "list_mcp_resource_templates"
            | "read_mcp_resource"
            | "git_workspace_info"
            | "git_status"
            | "git_diff" => Some(Self::Read),
            "write_file" | "apply_patch" | "create_directory" | "delete_path" | "copy_path"
            | "move_path" | "skill_manage" => Some(Self::WorkspaceWrite),
            "bash"
            | "write_stdin"
            | "container_exec"
            | "container_copy_to"
            | "container_copy_from" => Some(Self::Process),
            "spawn_agent" | "wait_agent" | "list_agents" | "send_input" | "close_agent"
            | "resume_agent" => Some(Self::AgentControl),
            "git_fetch"
            | "git_push"
            | "git_sync_default_branch"
            | "git_branch"
            | "git_commit"
            | "submit_delivery"
            | "task_merge_agent"
            | "task_update_design"
            | "task_request_review"
            | "task_complete"
            | "task_stop" => Some(Self::BranchControl),
            "review_exit" => Some(Self::Read),
            "merge_list_conflicts"
            | "merge_read_conflict"
            | "merge_resolve_file"
            | "merge_verify"
            | "merge_continue"
            | "merge_abort" => Some(Self::ConflictWrite),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_profiles_enforce_effect_and_owner_boundaries() {
        let planner = TurnExecutionProfile::root(CompileMode::Task);
        assert!(planner.allows_tool("read_file", Some(ToolEffect::Read)));
        assert!(planner.allows_tool("spawn_agent", Some(ToolEffect::AgentControl)));
        assert!(!planner.allows_tool("bash", Some(ToolEffect::Process)));
        assert!(!planner.allows_tool("dynamic", None));

        let explorer = TurnExecutionProfile::for_subagent(CompileMode::Task, "explorer");
        assert!(explorer.allows_tool("read_file", Some(ToolEffect::Read)));
        assert!(!explorer.allows_tool("spawn_agent", Some(ToolEffect::AgentControl)));
        assert!(!explorer.allows_tool("write_file", Some(ToolEffect::WorkspaceWrite)));

        let executor = TurnExecutionProfile::for_subagent(CompileMode::Task, "executor");
        assert!(executor.allows_tool("write_file", Some(ToolEffect::WorkspaceWrite)));
        assert!(executor.allows_tool("bash", Some(ToolEffect::Process)));
        assert!(executor.allows_tool("dynamic", None));
        assert!(!executor.allows_tool("spawn_agent", Some(ToolEffect::AgentControl)));

        let reviewer = TurnExecutionProfile::for_subagent(CompileMode::Task, "reviewer");
        assert!(reviewer.allows_tool("review_exit", Some(ToolEffect::Read)));
        assert!(!reviewer.allows_tool("dynamic", None));
        assert!(!reviewer.allows_tool("apply_patch", Some(ToolEffect::WorkspaceWrite)));
    }

    #[test]
    fn conflict_effect_requires_explicit_planner_phase() {
        let planner = TurnExecutionProfile::root(CompileMode::Task);
        assert!(!planner.allows_tool("merge_resolve_file", Some(ToolEffect::ConflictWrite)));
        assert!(
            planner
                .with_conflict_resolution()
                .allows_tool("merge_resolve_file", Some(ToolEffect::ConflictWrite))
        );
    }
}

/// 当前 turn 的代理职责。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnExecutionRole {
    Planner,
    Explorer,
    Executor,
    Reviewer,
}

impl TurnExecutionRole {
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "planner" => Some(Self::Planner),
            "explorer" => Some(Self::Explorer),
            "executor" => Some(Self::Executor),
            "reviewer" => Some(Self::Reviewer),
            _ => None,
        }
    }
}

/// 模式、角色和代理层级共同形成的工具执行档案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnExecutionProfile {
    mode: CompileMode,
    role: TurnExecutionRole,
    root_owner: bool,
    resolving_conflict: bool,
}

impl TurnExecutionProfile {
    pub fn root(mode: CompileMode) -> Self {
        let role = match mode {
            CompileMode::Simple => TurnExecutionRole::Executor,
            CompileMode::Task => TurnExecutionRole::Planner,
        };
        Self {
            mode,
            role,
            root_owner: true,
            resolving_conflict: false,
        }
    }

    pub fn for_subagent(mode: CompileMode, role: &str) -> Self {
        Self {
            mode,
            role: TurnExecutionRole::from_key(role).unwrap_or(TurnExecutionRole::Reviewer),
            root_owner: false,
            resolving_conflict: false,
        }
    }

    pub fn with_conflict_resolution(mut self) -> Self {
        self.resolving_conflict = true;
        self
    }

    pub fn role(self) -> TurnExecutionRole {
        self.role
    }

    pub fn is_root_owner(self) -> bool {
        self.root_owner
    }

    pub fn allows_tool(self, name: &str, effect: Option<ToolEffect>) -> bool {
        if name == "plan_exit" {
            return self.mode == CompileMode::Task
                && self.root_owner
                && self.role == TurnExecutionRole::Planner;
        }
        match self.role {
            TurnExecutionRole::Executor => match effect {
                Some(ToolEffect::Read | ToolEffect::WorkspaceWrite | ToolEffect::Process) => true,
                Some(ToolEffect::BranchControl) => true,
                Some(ToolEffect::AgentControl) => {
                    self.root_owner && self.mode == CompileMode::Simple
                }
                Some(ToolEffect::ConflictWrite) => false,
                None => true,
            },
            TurnExecutionRole::Planner => match effect {
                Some(ToolEffect::Read | ToolEffect::AgentControl) => self.root_owner,
                Some(ToolEffect::BranchControl) => {
                    self.root_owner && (name.starts_with("task_") || name.starts_with("merge_"))
                }
                Some(ToolEffect::ConflictWrite) => self.root_owner && self.resolving_conflict,
                Some(ToolEffect::WorkspaceWrite | ToolEffect::Process) | None => false,
            },
            TurnExecutionRole::Explorer | TurnExecutionRole::Reviewer => {
                matches!(effect, Some(ToolEffect::Read))
            }
        }
    }
}

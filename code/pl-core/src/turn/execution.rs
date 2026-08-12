/// 工具对运行环境产生的副作用类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolEffect {
    Read,
    WorkspaceWrite,
    Process,
    AgentControl,
    BranchControl,
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
            | "stat_path"
            | "skills_list"
            | "skill_view"
            | "request_user_input"
            | "update_todo_list"
            | "read_session_note"
            | "search_session_note"
            | "write_session_note"
            | "apply_session_note_patch"
            | "plan_exit"
            | "list_mcp_resources"
            | "list_mcp_resource_templates"
            | "read_mcp_resource"
            | "git_workspace_info"
            | "git_status"
            | "git_diff"
            | "task_status" => Some(Self::Read),
            "write_file" | "apply_patch" | "create_directory" | "delete_path" | "copy_path"
            | "move_path" | "skill_manage" => Some(Self::WorkspaceWrite),
            "exec" | "write_stdin" => Some(Self::Process),
            "spawn_agent" | "report_progress" | "send_message" | "interrupt_agent"
            | "list_agents" | "wait_agents" | "read_agent_session" | "close_agent" => {
                Some(Self::AgentControl)
            }
            "git_fetch"
            | "git_push"
            | "git_sync_default_branch"
            | "git_branch"
            | "git_commit"
            | "report_completion"
            | "task_record_merge"
            | "task_update_design"
            | "task_request_delivery_review"
            | "task_request_integrated_review"
            | "task_complete"
            | "task_stop" => Some(Self::BranchControl),
            "review_exit" => Some(Self::Read),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ToolEffect;

    #[test]
    fn removed_merge_tools_have_no_builtin_effect() {
        for name in [
            "merge_list_conflicts",
            "merge_read_conflict",
            "merge_resolve_file",
            "merge_verify",
            "merge_continue",
            "merge_abort",
        ] {
            assert_eq!(ToolEffect::for_builtin_name(name), None, "{name}");
        }
        assert_eq!(
            ToolEffect::for_builtin_name("task_record_merge"),
            Some(ToolEffect::BranchControl)
        );
    }
}

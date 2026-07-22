/// 工具对运行环境产生的副作用类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
            | "git_diff" => Some(Self::Read),
            "write_file" | "apply_patch" | "create_directory" | "delete_path" | "copy_path"
            | "move_path" | "skill_manage" => Some(Self::WorkspaceWrite),
            "exec" | "write_stdin" => Some(Self::Process),
            "spawn_agent" | "wait_agent" | "list_agents" | "send_input" | "close_agent" => {
                Some(Self::AgentControl)
            }
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

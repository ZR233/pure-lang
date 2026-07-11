use super::*;
use pretty_assertions::assert_eq;

#[test]
fn execution_modes_enforce_root_tool_effect_boundaries() {
    let simple = crate::turn::CompileMode::Simple;
    let task = crate::turn::CompileMode::Task;

    assert!(tool_allowed_in_mode(simple, "write_file"));
    assert!(tool_allowed_in_mode(simple, "mcp__github__search_issues"));
    assert!(tool_allowed_in_mode(task, "read_file"));
    assert!(tool_allowed_in_mode(task, "search_files"));
    assert!(tool_allowed_in_mode(task, "skills_list"));
    assert!(tool_allowed_in_mode(task, "skill_view"));
    assert!(tool_allowed_in_mode(task, "spawn_agent"));
    assert!(tool_allowed_in_mode(task, "send_input"));
    assert!(tool_allowed_in_mode(task, "request_user_input"));
    assert!(tool_allowed_in_mode(task, "update_todo_list"));
    assert!(tool_allowed_in_mode(task, "lsp_query_rust"));
    assert!(!tool_allowed_in_mode(task, "bash"));
    assert!(!tool_allowed_in_mode(task, "mcp__github__search_issues"));
    assert!(!tool_allowed_in_mode(task, "subagent"));
    assert!(!tool_allowed_in_mode(task, "write_file"));
    assert!(!tool_allowed_in_mode(task, "apply_patch"));
    assert!(!tool_allowed_in_mode(task, "delete_path"));
    assert!(!tool_allowed_in_mode(task, "skill_manage"));
}

#[test]
fn tool_trace_part_ids_are_scoped_to_turn() {
    assert_eq!(
        namespaced_tool_trace_part_id("turn-1", "call_0"),
        "turn-1-call_0"
    );
    assert_eq!(
        namespaced_tool_trace_part_id("turn-1", "turn-1-call_0"),
        "turn-1-call_0"
    );
}

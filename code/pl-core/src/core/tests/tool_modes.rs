use super::*;
use pretty_assertions::assert_eq;

#[test]
fn plan_mode_tool_allowlist_exposes_only_read_and_agent_tools() {
    let auto = crate::turn::CompileMode::Auto;
    let plan = crate::turn::CompileMode::Plan;

    assert!(tool_allowed_in_mode(auto, "write_file"));
    assert!(tool_allowed_in_mode(plan, "read_file"));
    assert!(tool_allowed_in_mode(plan, "search_files"));
    assert!(tool_allowed_in_mode(plan, "skills_list"));
    assert!(tool_allowed_in_mode(plan, "skill_view"));
    assert!(tool_allowed_in_mode(plan, "spawn_agent"));
    assert!(tool_allowed_in_mode(plan, "followup_task"));
    assert!(tool_allowed_in_mode(plan, "request_user_input"));
    assert!(tool_allowed_in_mode(plan, "bash"));
    assert!(tool_allowed_in_mode(plan, "lsp_query_rust"));
    assert!(tool_allowed_in_mode(plan, "mcp__github__search_issues"));
    assert!(!tool_allowed_in_mode(plan, "subagent"));
    assert!(!tool_allowed_in_mode(plan, "write_file"));
    assert!(!tool_allowed_in_mode(plan, "apply_patch"));
    assert!(!tool_allowed_in_mode(plan, "delete_path"));
    assert!(!tool_allowed_in_mode(plan, "skill_manage"));
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

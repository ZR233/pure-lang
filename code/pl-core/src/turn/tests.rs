use pretty_assertions::assert_eq;

use super::*;

#[test]
fn turn_budget_default_matches_codex_style_wall_clock() {
    assert_eq!(TurnBudget::default().wall_clock_ms, 1_800_000);
    assert_eq!(TurnBudget::new(90_000).wall_clock_ms, 90_000);
    assert_eq!(AGENT_MAX_COUNT, 16);
    assert_eq!(AGENT_MAX_DEPTH, 3);
}

#[test]
fn turn_request_uses_default_budget() {
    let request = TurnRequest::new("hello", CompileMode::Auto);

    assert_eq!(request.budget, TurnBudget::default());
    assert_eq!(request.prompt, "hello");
    assert_eq!(request.mode, CompileMode::Auto);
}

#[test]
fn compile_mode_from_label_keeps_old_values_auto_compatible() {
    assert_eq!(CompileMode::from_label("plan"), CompileMode::Plan);
    assert_eq!(CompileMode::from_label("auto"), CompileMode::Auto);
    assert_eq!(CompileMode::from_label("manual"), CompileMode::Auto);
    assert_eq!(CompileMode::from_label(""), CompileMode::Auto);
}

#[test]
fn compile_mode_default_is_auto() {
    assert_eq!(CompileMode::default(), CompileMode::Auto);
}

#[test]
fn permission_mode_from_label_keeps_unknown_values_safe() {
    assert_eq!(
        PermissionMode::from_label("request-approval"),
        PermissionMode::RequestApproval
    );
    assert_eq!(
        PermissionMode::from_label("auto-review"),
        PermissionMode::AutoReview
    );
    assert_eq!(
        PermissionMode::from_label("workspace-write"),
        PermissionMode::RequestApproval
    );
    assert_eq!(
        PermissionMode::from_label("full-access"),
        PermissionMode::FullAccess
    );
    assert_eq!(
        PermissionMode::from_label("old-auto-allow"),
        PermissionMode::RequestApproval
    );
    assert!(PermissionMode::FullAccess.allows_workspace_escape());
    assert!(!PermissionMode::RequestApproval.allows_workspace_escape());
}

#[test]
fn turn_abort_reason_has_stable_wire_labels() {
    assert_eq!(TurnAbortReason::Interrupted.as_str(), "interrupted");
    assert_eq!(TurnAbortReason::BudgetLimited.as_str(), "budgetLimited");
    assert_eq!(TurnAbortReason::Shutdown.as_str(), "shutdown");
    assert_eq!(TurnAbortReason::ProviderError.as_str(), "providerError");
    assert_eq!(TurnAbortReason::ToolError.as_str(), "toolError");
}

#[test]
fn budget_tracker_records_observability() {
    let mut tracker = BudgetTracker::new(TurnBudget::new(60_000));

    tracker.record_model_step();
    tracker.record_tool_call("bash");
    tracker.record_tool_call("wait_agent");

    let usage = tracker.usage();
    assert_eq!(usage.model_steps, 1);
    assert_eq!(usage.tool_calls, 1);
    assert_eq!(usage.wait_calls, 1);
}

#[test]
fn budget_tracker_only_enforces_wall_clock() {
    let mut tracker = BudgetTracker::new(TurnBudget::new(60_000));

    for _ in 0..200 {
        tracker.record_model_step();
        tracker.record_tool_call("bash");
    }

    assert!(tracker.check_wall_clock().is_ok());

    let usage = tracker.usage();
    assert_eq!(usage.model_steps, 200);
    assert_eq!(usage.tool_calls, 200);
}

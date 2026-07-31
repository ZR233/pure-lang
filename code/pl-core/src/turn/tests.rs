use pretty_assertions::assert_eq;

use super::*;

#[test]
fn permission_mode_accepts_only_current_labels() {
    assert_eq!(
        PermissionMode::from_label("request-approval"),
        Some(PermissionMode::RequestApproval)
    );
    assert_eq!(
        PermissionMode::from_label("auto-review"),
        Some(PermissionMode::AutoReview)
    );
    assert_eq!(
        PermissionMode::from_label("full-access"),
        Some(PermissionMode::FullAccess)
    );
    assert_eq!(PermissionMode::from_label("workspace-write"), None);
    assert_eq!(PermissionMode::from_label("old-auto-allow"), None);
    assert!(PermissionMode::FullAccess.allows_workspace_escape());
    assert!(!PermissionMode::RequestApproval.allows_workspace_escape());
}

#[test]
fn budget_tracker_records_observability() {
    let mut tracker = BudgetTracker::new(TurnBudget::new(60_000));

    tracker.record_model_step();
    tracker.record_tool_call("exec");
    tracker.record_tool_call("list_agents");

    let usage = tracker.usage();
    assert_eq!(usage.model_steps, 1);
    assert_eq!(usage.tool_calls, 2);
    assert_eq!(usage.wait_calls, 0);
}

#[test]
fn budget_tracker_only_enforces_wall_clock() {
    let mut tracker = BudgetTracker::new(TurnBudget::new(60_000));

    for _ in 0..200 {
        tracker.record_model_step();
        tracker.record_tool_call("exec");
    }

    assert!(tracker.check_wall_clock().is_ok());

    let usage = tracker.usage();
    assert_eq!(usage.model_steps, 200);
    assert_eq!(usage.tool_calls, 200);
}

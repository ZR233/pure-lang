use pretty_assertions::assert_eq;
use std::time::{Duration, Instant};

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
    tracker.record_tool_call("wait_agents");

    let usage = tracker.usage();
    assert_eq!(usage.model_steps, 1);
    assert_eq!(usage.tool_calls, 2);
    assert_eq!(usage.wait_calls, 1);
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

#[test]
fn budget_tracker_stops_when_active_wall_clock_reaches_limit() {
    let tracker = BudgetTracker::new(TurnBudget::new(0));

    assert!(tracker.check_wall_clock().is_err());
}

#[test]
fn budget_refresh_resets_time_exclusions_and_tranche_counts() {
    let mut tracker = BudgetTracker::new(TurnBudget::new(60_000));
    tracker.record_model_step();
    tracker.record_tool_call("exec");
    tracker.record_tool_call("wait_agents");
    tracker.exclude_wall_clock(Duration::from_secs(30));

    tracker.refresh_at(Instant::now() - Duration::from_millis(5));

    let usage = tracker.usage();
    assert_eq!(usage.model_steps, 0);
    assert_eq!(usage.tool_calls, 0);
    assert_eq!(usage.wait_calls, 0);
    assert!(usage.elapsed_ms >= 5);
    assert!(usage.elapsed_ms < 1_000);
}

#[test]
fn turn_options_consumes_only_the_latest_budget_refresh_once() {
    let (refresh, receiver) = crate::agent_runtime::turn_budget_refresh_channel();
    let mut options = TurnOptions::default();
    options.budget_refresh = Some(receiver);
    let mut tracker = BudgetTracker::new(TurnBudget::new(60_000));
    tracker.record_model_step();

    refresh.refresh();
    refresh.refresh();
    options.apply_budget_refresh(&mut tracker);
    assert_eq!(tracker.usage().model_steps, 0);

    tracker.record_model_step();
    options.apply_budget_refresh(&mut tracker);
    assert_eq!(tracker.usage().model_steps, 1);
}

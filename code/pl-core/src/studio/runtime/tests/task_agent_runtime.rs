use std::path::Path;

use super::super::TaskAgentRuntimeRegistry;
use crate::CompileMode;

#[tokio::test]
async fn task_sessions_reuse_only_their_own_supervisor() {
    let registry = TaskAgentRuntimeRegistry::new();
    let repository = Path::new("C:/repo");

    let first = registry
        .supervisor_for_task("session-a", repository, 7)
        .await
        .unwrap();
    let continuation = registry
        .supervisor_for_task("session-a", repository, 7)
        .await
        .unwrap();
    let other_session = registry
        .supervisor_for_task("session-b", repository, 7)
        .await
        .unwrap();

    assert!(first.shares_runtime_with(&continuation));
    assert!(!first.shares_runtime_with(&other_session));
    assert_eq!(registry.len().await, 2);
}

#[tokio::test]
async fn simple_mode_remains_turn_local() {
    let registry = TaskAgentRuntimeRegistry::new();

    let first = registry
        .supervisor_for_mode(CompileMode::Simple, "session", Path::new("C:/repo"), 1)
        .await
        .unwrap();
    let second = registry
        .supervisor_for_mode(CompileMode::Simple, "session", Path::new("C:/repo"), 1)
        .await
        .unwrap();

    assert!(first.is_none());
    assert!(second.is_none());
    assert_eq!(registry.len().await, 0);
}

#[tokio::test]
async fn repository_or_epoch_drift_is_rejected_without_replacing_runtime() {
    let registry = TaskAgentRuntimeRegistry::new();
    let original = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 3)
        .await
        .unwrap();

    let repository_error = registry
        .supervisor_for_task("session", Path::new("C:/other"), 3)
        .await
        .unwrap_err();
    let epoch_error = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 4)
        .await
        .unwrap_err();
    let still_original = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 3)
        .await
        .unwrap();

    assert!(repository_error.to_string().contains("repository"));
    assert!(epoch_error.to_string().contains("epoch"));
    assert!(original.shares_runtime_with(&still_original));
}

#[tokio::test]
async fn shutdown_quiesces_then_clears_registry_for_next_epoch() {
    let registry = TaskAgentRuntimeRegistry::new();
    let before = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 2)
        .await
        .unwrap();

    registry.quiesce_and_clear().await.unwrap();

    assert_eq!(registry.len().await, 0);
    let after = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 3)
        .await
        .unwrap();
    assert!(!before.shares_runtime_with(&after));
}

#[tokio::test]
async fn planning_generation_binds_first_run_then_rotates_after_terminal() {
    let registry = TaskAgentRuntimeRegistry::new();
    let planning = registry
        .supervisor_for_task_generation("session", Path::new("C:/repo"), 5, None)
        .await
        .unwrap();
    let first_run = registry
        .supervisor_for_task_generation("session", Path::new("C:/repo"), 5, Some("run-1"))
        .await
        .unwrap();
    let same_run = registry
        .supervisor_for_task_generation("session", Path::new("C:/repo"), 5, Some("run-1"))
        .await
        .unwrap();
    let next_planning = registry
        .supervisor_for_task_generation("session", Path::new("C:/repo"), 5, None)
        .await
        .unwrap();

    assert!(planning.shares_runtime_with(&first_run));
    assert!(first_run.shares_runtime_with(&same_run));
    assert!(!first_run.shares_runtime_with(&next_planning));
    assert_eq!(registry.len().await, 1);
}

use super::*;
use crate::studio::task_coordinator::{TaskStopOrigin, TaskStopReason};

#[test]
fn lifecycle_transition_table_accepts_every_declared_path() {
    let cases = vec![
        (
            state(TaskRunStateKind::DesignUpdating),
            TaskCommand::FinalizeDesign(finalized_design()),
            TaskRunStateKind::Implementing,
        ),
        (
            state(TaskRunStateKind::Merging),
            TaskCommand::BeginImplementing,
            TaskRunStateKind::Implementing,
        ),
        (
            state(TaskRunStateKind::Reworking),
            TaskCommand::BeginImplementing,
            TaskRunStateKind::Implementing,
        ),
        (
            state(TaskRunStateKind::Implementing),
            TaskCommand::BeginMerging {
                status_message: None,
            },
            TaskRunStateKind::Merging,
        ),
        (
            state(TaskRunStateKind::Reworking),
            TaskCommand::BeginMerging {
                status_message: None,
            },
            TaskRunStateKind::Merging,
        ),
        (
            state(TaskRunStateKind::Implementing),
            TaskCommand::BeginReviewing(review_target()),
            TaskRunStateKind::Reviewing,
        ),
        (
            state(TaskRunStateKind::Merging),
            TaskCommand::BeginReviewing(review_target()),
            TaskRunStateKind::Reviewing,
        ),
        (
            state(TaskRunStateKind::Reworking),
            TaskCommand::BeginReviewing(review_target()),
            TaskRunStateKind::Reviewing,
        ),
        (
            state(TaskRunStateKind::Reviewing),
            TaskCommand::BeginReworking {
                status_message: "review changes".to_string(),
            },
            TaskRunStateKind::Reworking,
        ),
        (
            state(TaskRunStateKind::Merging),
            TaskCommand::BeginReworking {
                status_message: "merge changes".to_string(),
            },
            TaskRunStateKind::Reworking,
        ),
    ];

    for (source, command, expected) in cases {
        let actual = source.kind();
        let decision = source.decide(command).unwrap_or_else(|error| {
            panic!("{actual:?} should transition to {expected:?}: {error:#}")
        });
        assert_eq!(decision.next_state.kind(), expected);
    }
}

#[test]
fn every_nonterminal_state_can_stop_block_fail_and_cancel() {
    for kind in nonterminal_kinds() {
        let stopping = state(kind)
            .decide(TaskCommand::RequestStop(stop_request()))
            .unwrap();
        assert_eq!(stopping.next_state.kind(), TaskRunStateKind::Stopping);
        assert_eq!(
            stopping.external_effects,
            vec![TaskExternalEffect::InterruptAgents]
        );

        let blocked = state(kind)
            .decide(TaskCommand::Block {
                message: "blocked".to_string(),
                recovery: BlockedRecovery::ManualOnly,
            })
            .unwrap();
        assert_eq!(blocked.next_state.kind(), TaskRunStateKind::Blocked);

        let failed = state(kind)
            .decide(TaskCommand::Fail {
                message: "failed".to_string(),
                failure_id: Some("failure-1".to_string()),
            })
            .unwrap();
        assert_eq!(failed.next_state.kind(), TaskRunStateKind::Failed);
        assert_eq!(
            failed.durable_effects,
            vec![TaskDurableEffect::ReleaseProjectLease]
        );

        let cancelled = state(kind)
            .decide(TaskCommand::Cancel {
                message: "cancelled".to_string(),
                request: None,
            })
            .unwrap();
        assert_eq!(cancelled.next_state.kind(), TaskRunStateKind::Cancelled);
    }
}

#[test]
fn completion_is_limited_to_delivery_bearing_states() {
    for kind in [
        TaskRunStateKind::Implementing,
        TaskRunStateKind::Reviewing,
        TaskRunStateKind::Reworking,
    ] {
        let decision = state(kind).decide(TaskCommand::Complete).unwrap();
        assert_eq!(decision.next_state.kind(), TaskRunStateKind::Completed);
        assert_eq!(
            decision.durable_effects,
            vec![TaskDurableEffect::ReleaseProjectLease]
        );
    }
    for kind in [
        TaskRunStateKind::DesignUpdating,
        TaskRunStateKind::Merging,
        TaskRunStateKind::Stopping,
        TaskRunStateKind::Blocked,
    ] {
        assert!(
            state(kind).decide(TaskCommand::Complete).is_err(),
            "{kind:?}"
        );
    }
}

#[test]
fn blocked_recovery_is_typed_and_increments_generation() {
    let retry = TaskRunState::Blocked(BlockedState::new(
        DesignProgress::from_finalized(finalized_design()),
        7,
        "retry merge".to_string(),
        BlockedRecovery::RetryMerge,
    ));
    let retried = retry
        .decide(TaskCommand::RecoverBlocked {
            recovery: BlockedRecovery::RetryMerge,
            status_message: "retry merge".to_string(),
        })
        .unwrap();
    assert_eq!(retried.next_state.kind(), TaskRunStateKind::Merging);
    assert_eq!(retried.next_state.generation(), 8);

    let resume = TaskRunState::Blocked(BlockedState::new(
        DesignProgress::from_finalized(finalized_design()),
        11,
        "resume rework".to_string(),
        BlockedRecovery::ResumeRework,
    ));
    let resumed = resume
        .decide(TaskCommand::RecoverBlocked {
            recovery: BlockedRecovery::ResumeRework,
            status_message: "resume after operator repair".to_string(),
        })
        .unwrap();
    assert_eq!(resumed.next_state.kind(), TaskRunStateKind::Reworking);
    assert_eq!(resumed.next_state.generation(), 12);

    let manual = state(TaskRunStateKind::Blocked);
    assert!(
        manual
            .clone()
            .decide(TaskCommand::RecoverBlocked {
                recovery: BlockedRecovery::RetryMerge,
                status_message: "retry".to_string(),
            })
            .is_err()
    );
    assert!(
        manual
            .decide(TaskCommand::RecoverBlocked {
                recovery: BlockedRecovery::ResumeRework,
                status_message: "not allowed".to_string(),
            })
            .is_err()
    );
}

#[test]
fn terminal_states_reject_every_command() {
    for kind in [
        TaskRunStateKind::Completed,
        TaskRunStateKind::Failed,
        TaskRunStateKind::Cancelled,
    ] {
        for command in commands() {
            assert!(
                state(kind).decide(command).is_err(),
                "{kind:?} accepted a command"
            );
        }
    }
}

#[test]
fn every_state_payload_round_trips_with_one_canonical_discriminator() {
    for kind in all_kinds() {
        let state = state(kind);
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["kind"], kind.as_str());
        let decoded: TaskRunState = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, state);
    }
}

fn state(kind: TaskRunStateKind) -> TaskRunState {
    let design = finalized_design();
    let progress = DesignProgress::from_finalized(design.clone());
    match kind {
        TaskRunStateKind::DesignUpdating => TaskRunState::new(),
        TaskRunStateKind::Implementing => {
            TaskRunState::Implementing(ImplementingState::new(design, 3))
        }
        TaskRunStateKind::Merging => {
            TaskRunState::Merging(MergingState::new(design, 3, Some("merging".to_string())))
        }
        TaskRunStateKind::Reviewing => {
            TaskRunState::Reviewing(ReviewingState::new(design, 3, review_target()))
        }
        TaskRunStateKind::Reworking => {
            TaskRunState::Reworking(ReworkingState::new(design, 3, "rework".to_string()))
        }
        TaskRunStateKind::Stopping => {
            TaskRunState::Stopping(StoppingState::new(progress, 4, stop_request()))
        }
        TaskRunStateKind::Blocked => TaskRunState::Blocked(BlockedState::new(
            progress,
            3,
            "blocked".to_string(),
            BlockedRecovery::ManualOnly,
        )),
        TaskRunStateKind::Completed => TaskRunState::Completed(CompletedState::new(design, 3)),
        TaskRunStateKind::Failed => TaskRunState::Failed(FailedState::new(
            progress,
            3,
            "failed".to_string(),
            Some("failure-1".to_string()),
        )),
        TaskRunStateKind::Cancelled => TaskRunState::Cancelled(CancelledState::new(
            progress,
            3,
            "cancelled".to_string(),
            Some(stop_request()),
        )),
    }
}

fn finalized_design() -> FinalizedDesign {
    FinalizedDesign {
        summary: "design summary".to_string(),
    }
}

fn review_target() -> ReviewTarget {
    ReviewTarget::Integration {
        reviewed_head: "2222222".to_string(),
    }
}

fn stop_request() -> TaskStopRequest {
    TaskStopRequest {
        origin: TaskStopOrigin::UserRequest,
        reason: TaskStopReason::new("stop").unwrap(),
        requested_at: 10,
    }
}

fn nonterminal_kinds() -> [TaskRunStateKind; 7] {
    [
        TaskRunStateKind::DesignUpdating,
        TaskRunStateKind::Implementing,
        TaskRunStateKind::Merging,
        TaskRunStateKind::Reviewing,
        TaskRunStateKind::Reworking,
        TaskRunStateKind::Stopping,
        TaskRunStateKind::Blocked,
    ]
}

fn all_kinds() -> [TaskRunStateKind; 10] {
    [
        TaskRunStateKind::DesignUpdating,
        TaskRunStateKind::Implementing,
        TaskRunStateKind::Merging,
        TaskRunStateKind::Reviewing,
        TaskRunStateKind::Reworking,
        TaskRunStateKind::Stopping,
        TaskRunStateKind::Blocked,
        TaskRunStateKind::Completed,
        TaskRunStateKind::Failed,
        TaskRunStateKind::Cancelled,
    ]
}

fn commands() -> Vec<TaskCommand> {
    vec![
        TaskCommand::FinalizeDesign(finalized_design()),
        TaskCommand::BeginImplementing,
        TaskCommand::BeginMerging {
            status_message: None,
        },
        TaskCommand::BeginReviewing(review_target()),
        TaskCommand::BeginReworking {
            status_message: "rework".to_string(),
        },
        TaskCommand::RequestStop(stop_request()),
        TaskCommand::Block {
            message: "blocked".to_string(),
            recovery: BlockedRecovery::ManualOnly,
        },
        TaskCommand::RecoverBlocked {
            recovery: BlockedRecovery::RetryMerge,
            status_message: "resume".to_string(),
        },
        TaskCommand::Complete,
        TaskCommand::Fail {
            message: "failed".to_string(),
            failure_id: None,
        },
        TaskCommand::Cancel {
            message: "cancelled".to_string(),
            request: None,
        },
    ]
}

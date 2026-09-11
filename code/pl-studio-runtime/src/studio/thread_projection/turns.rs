//! Turn lifecycle and diagnostics projected from saved commit metadata.
use super::ProjectionError;
use pl_core::thread::{
    AttemptOutcome, ThreadSnapshot, TurnOutcome as CoreTurnOutcome, TurnRecord,
    TurnState as CoreTurnState, journal::ThreadCommit,
};
use pl_protocol::{Turn, TurnPhase, TurnState};
use std::{collections::BTreeMap, sync::Arc};

struct Stamp {
    started_at: i64,
    updated_at: i64,
    revision: u64,
}

pub(in crate::studio) fn project_turns(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadCommit>],
) -> Result<Vec<Turn>, ProjectionError> {
    let mut stamps = BTreeMap::<String, Stamp>::new();
    for commit in journal
        .iter()
        .filter(|commit| commit.sequence <= snapshot.commit_sequence)
    {
        if let Some(turn) = &commit.turn {
            let stamp = stamps.entry(turn.turn_id.clone()).or_insert(Stamp {
                started_at: commit.committed_at,
                updated_at: commit.committed_at,
                revision: commit.sequence,
            });
            stamp.updated_at = commit.committed_at;
            stamp.revision = commit.sequence;
        }
        if let Some(attempt) = &commit.attempt
            && let Some(stamp) = stamps.get_mut(&attempt.turn_id)
        {
            stamp.updated_at = commit.committed_at;
            stamp.revision = commit.sequence;
        }
        for task in commit.tasks.iter() {
            if let Some(stamp) = stamps.get_mut(&task.turn_id)
                && snapshot.turns.iter().any(|turn| {
                    turn.turn_id == task.turn_id && turn.state == CoreTurnState::Running
                })
            {
                stamp.updated_at = commit.committed_at;
                stamp.revision = commit.sequence;
            }
        }
    }
    snapshot
        .turns
        .iter()
        .map(|record| {
            let stamp = stamps
                .get(&record.turn_id)
                .ok_or_else(|| ProjectionError::MissingTurn(record.turn_id.clone()))?;
            let state = state(snapshot, record, stamp)?;
            Ok(Turn {
                input_id: record.input_id.clone(),
                id: record.turn_id.clone(),
                thread_id: thread_id.into(),
                revision: stamp.revision,
                state,
                updated_at: stamp.updated_at,
            })
        })
        .collect()
}

fn state(
    snapshot: &ThreadSnapshot,
    record: &TurnRecord,
    stamp: &Stamp,
) -> Result<TurnState, ProjectionError> {
    let started = Some(stamp.started_at);
    let at = stamp.updated_at;
    Ok(match &record.state {
        CoreTurnState::Running => TurnState::Running(pl_protocol::RunningTurnState::new(
            stamp.started_at,
            phase(snapshot, &record.turn_id),
        )),
        CoreTurnState::Finished(CoreTurnOutcome::Completed | CoreTurnOutcome::ToolCompleted) => {
            TurnState::Completed(pl_protocol::CompletedTurnState::new(
                started,
                at,
                pl_protocol::TurnCompletion::Normal,
            ))
        }
        CoreTurnState::Finished(CoreTurnOutcome::WaitingInteraction) => {
            TurnState::Completed(pl_protocol::CompletedTurnState::new(
                started,
                at,
                pl_protocol::TurnCompletion::InteractionRequested,
            ))
        }
        CoreTurnState::Finished(CoreTurnOutcome::StepLimit) => {
            let tasks = snapshot
                .tasks
                .values()
                .filter(|task| task.turn_id == record.turn_id)
                .collect::<Vec<_>>();
            let usage = pl_protocol::BudgetUsage {
                model_steps: record.model_steps,
                tool_calls: tasks.len().try_into().map_err(|_| ProjectionError::Count)?,
                wait_calls: tasks
                    .iter()
                    .filter(|task| task.tool_id == "wait")
                    .count()
                    .try_into()
                    .map_err(|_| ProjectionError::Count)?,
                elapsed_ms: record
                    .elapsed_ms
                    .ok_or_else(|| ProjectionError::MissingDuration(record.turn_id.clone()))?,
            };
            TurnState::BudgetLimited(pl_protocol::BudgetLimitedTurnState::new(
                started,
                at,
                pl_protocol::BudgetLimitSnapshot {
                    kind: pl_protocol::BudgetLimitKind::ModelStep,
                    usage,
                },
                pl_protocol::TurnRolloverOutcome::NotAttempted,
            ))
        }
        CoreTurnState::Cancelled => TurnState::Cancelled(pl_protocol::CancelledTurnState::new(
            started,
            at,
            at,
            pl_protocol::TurnCancellationCause::Unspecified,
        )),
        CoreTurnState::Interrupted => TurnState::Cancelled(pl_protocol::CancelledTurnState::new(
            started,
            at,
            at,
            pl_protocol::TurnCancellationCause::Recovery,
        )),
        CoreTurnState::Failed { description } => {
            TurnState::Failed(pl_protocol::FailedTurnState::new(
                started,
                at,
                failure(snapshot, &record.turn_id, description),
            ))
        }
    })
}

fn phase(snapshot: &ThreadSnapshot, turn_id: &str) -> TurnPhase {
    if snapshot.tasks.values().any(|task| {
        task.turn_id == turn_id && task.status == pl_core::thread::task::TaskStatus::Running
    }) {
        return TurnPhase::RunningTool;
    }
    match snapshot
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.turn_id == turn_id)
        .map(|attempt| &attempt.outcome)
    {
        None => TurnPhase::Preparing,
        Some(AttemptOutcome::Committed(output)) if !output.tool_calls.is_empty() => {
            TurnPhase::Planning
        }
        Some(AttemptOutcome::Committed(_)) => TurnPhase::Responding,
        Some(
            AttemptOutcome::Running
            | AttemptOutcome::Interrupted
            | AttemptOutcome::Cancelled { .. }
            | AttemptOutcome::Failed(_)
            | AttemptOutcome::Rejected { .. },
        ) => TurnPhase::Thinking,
    }
}

fn failure(
    snapshot: &ThreadSnapshot,
    turn_id: &str,
    description: &str,
) -> pl_protocol::TurnFailure {
    let Some(error) = snapshot
        .attempts
        .iter()
        .rev()
        .filter(|attempt| attempt.turn_id == turn_id)
        .find_map(|attempt| match &attempt.outcome {
            AttemptOutcome::Failed(error) => Some(error),
            AttemptOutcome::Running
            | AttemptOutcome::Interrupted
            | AttemptOutcome::Committed(_)
            | AttemptOutcome::Cancelled { .. }
            | AttemptOutcome::Rejected { .. } => None,
        })
    else {
        return pl_protocol::TurnFailure::permanent(
            pl_protocol::TurnFailureCategory::Internal,
            description,
        );
    };
    match pl_model::runtime::model_failure_receipt(error) {
        Ok(Some(receipt)) => match receipt.provider_failure {
            Some(failure) => pl_protocol::TurnFailure {
                category: if failure.kind == pl_protocol::ProviderFailureKind::Capacity {
                    pl_protocol::TurnFailureCategory::ProviderCapacity
                } else {
                    pl_protocol::TurnFailureCategory::Provider
                },
                provider_kind: Some(failure.kind),
                code: failure.code,
                http_status: failure.http_status,
                message: failure.message,
                retry: failure.retry,
            },
            None => pl_protocol::TurnFailure::permanent(
                pl_protocol::TurnFailureCategory::Provider,
                if receipt.message.is_empty() {
                    description.to_owned()
                } else {
                    receipt.message
                },
            ),
        },
        Ok(None) => pl_protocol::TurnFailure::permanent(
            pl_protocol::TurnFailureCategory::Provider,
            description,
        ),
        Err(error) => pl_protocol::TurnFailure::permanent(
            pl_protocol::TurnFailureCategory::Protocol,
            format!("{description}; saved model failure cannot be decoded: {error}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn interaction_completion_uses_saved_times_without_running_a_model() {
        let snapshot = ThreadSnapshot::default();
        let record = TurnRecord {
            input_id: Some("accepted-input".into()),
            elapsed_ms: Some(42),
            turn_id: "turn".into(),
            state: CoreTurnState::Finished(CoreTurnOutcome::WaitingInteraction),
            model_steps: 1,
        };
        let stamp = Stamp {
            started_at: 100,
            updated_at: 101,
            revision: 2,
        };
        let TurnState::Completed(state) = state(&snapshot, &record, &stamp).unwrap() else {
            panic!("expected interaction completion")
        };
        assert_eq!(state.started_at(), Some(100));
        assert_eq!(state.completed_at(), 101);
        assert_eq!(
            state.completion(),
            pl_protocol::TurnCompletion::InteractionRequested
        );
    }

    #[test]
    fn step_limit_uses_measured_duration_and_generic_cancel_does_not_invent_an_initiator() {
        let stamp = Stamp {
            started_at: 10,
            updated_at: 11,
            revision: 2,
        };
        let snapshot = ThreadSnapshot::default();
        let mut record = TurnRecord {
            input_id: None,
            elapsed_ms: Some(325),
            turn_id: "turn".into(),
            state: CoreTurnState::Finished(CoreTurnOutcome::StepLimit),
            model_steps: 4,
        };
        let TurnState::BudgetLimited(limited) = state(&snapshot, &record, &stamp).unwrap() else {
            panic!("expected budget limit")
        };
        assert_eq!(limited.limit().usage.elapsed_ms, 325);
        record.elapsed_ms = None;
        assert!(matches!(
            state(&snapshot, &record, &stamp),
            Err(ProjectionError::MissingDuration(_))
        ));
        record.state = CoreTurnState::Cancelled;
        let TurnState::Cancelled(cancelled) = state(&snapshot, &record, &stamp).unwrap() else {
            panic!("expected cancellation")
        };
        assert_eq!(
            cancelled.cause(),
            &pl_protocol::TurnCancellationCause::Unspecified
        );
    }
}

//! Turn lifecycle and diagnostics projected from saved commit metadata.
use super::ProjectionError;
use pl_core::thread::{
    AttemptOutcome, ThreadSnapshot, TurnOutcome as CoreTurnOutcome, TurnRecord,
    TurnState as CoreTurnState,
};
use pl_protocol::{Turn, TurnPhase, TurnState};

struct Stamp {
    started_at: i64,
    updated_at: i64,
    revision: u64,
}

pub(super) fn project_turn(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    record: &TurnRecord,
    created_at: i64,
    updated_at: i64,
    revision: u64,
) -> Result<Turn, ProjectionError> {
    let stamp = Stamp {
        started_at: created_at,
        updated_at,
        revision,
    };
    Ok(Turn {
        input_id: record.input_id.clone(),
        id: record.turn_id.clone(),
        thread_id: thread_id.into(),
        revision,
        state: state(snapshot, record, &stamp)?,
        updated_at,
    })
}

pub(in crate::studio) fn project_active_turn(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    updated_at: i64,
) -> Result<Option<Turn>, ProjectionError> {
    let Some(record) = snapshot
        .turns
        .iter()
        .rev()
        .find(|record| record.state == CoreTurnState::Running)
    else {
        return Ok(None);
    };
    let stamp = Stamp {
        started_at: updated_at,
        updated_at,
        revision: snapshot.commit_sequence,
    };
    Ok(Some(Turn {
        input_id: record.input_id.clone(),
        id: record.turn_id.clone(),
        thread_id: thread_id.into(),
        revision: stamp.revision,
        state: state(snapshot, record, &stamp)?,
        updated_at: stamp.updated_at,
    }))
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
        CoreTurnState::Finished(CoreTurnOutcome::Completed) => TurnState::Completed(
            pl_protocol::CompletedTurnState::new(started, at, pl_protocol::TurnCompletion::Normal),
        ),
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
            if record.elapsed_ms.is_some() {
                pl_protocol::TurnCancellationCause::Interrupted
            } else {
                pl_protocol::TurnCancellationCause::Recovery
            },
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

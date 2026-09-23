//! Authoritative current snapshots and independently persisted history projections.
use super::{ProjectionError, project_active_turn};
use pl_core::thread::{ThreadLifecycle, ThreadSnapshot, TurnState, input::InputState};
use pl_protocol::{Thread, ThreadStatus};

pub(in crate::studio) fn project_snapshot(
    mut thread: Thread,
    state: &ThreadSnapshot,
    usage: &pl_core::thread::UsageSummary,
) -> Result<pl_protocol::ThreadSnapshot, ProjectionError> {
    let active_turn = project_active_turn(&thread.id, state, thread.updated_at)?;
    let mut interactions = Vec::new();
    for id in state.permissions.keys().chain(state.interactions.keys()) {
        if let Ok(Some(interaction)) =
            crate::thread_assembler::project_thread_interaction(&thread.id, id, state)
        {
            interactions.push(interaction);
        }
    }
    thread.status = status(state);
    if let Ok(Some(mode)) = saved_mode(state) {
        thread.mode = mode;
    }
    let runtime = super::runtime::project_runtime(&thread.id, state, thread.updated_at, usage).ok();
    Ok(pl_protocol::ThreadSnapshot {
        schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
        revision: state.commit_sequence,
        runtime,
        thread,
        active_turn,
        interactions,
    })
}

pub(in crate::studio) fn status(state: &ThreadSnapshot) -> ThreadStatus {
    match state.lifecycle {
        ThreadLifecycle::Closing => ThreadStatus::Closing,
        ThreadLifecycle::Closed => ThreadStatus::Closed,
        ThreadLifecycle::Open => {
            if matches!(
                state.input_execution,
                pl_core::thread::input::InputExecution::Interrupting { .. }
            ) {
                return ThreadStatus::Cancelling;
            }
            if matches!(
                state.input_execution,
                pl_core::thread::input::InputExecution::Failed { .. }
            ) {
                return ThreadStatus::Faulted;
            }
            if state.interactions.values().any(|record| {
                record.state == pl_core::thread::interactions::InteractionState::Pending
            }) || state.permissions.values().any(|record| {
                record.state == pl_core::thread::permissions::PermissionState::Pending
            }) {
                ThreadStatus::WaitingInteraction
            } else if state
                .turns
                .iter()
                .any(|turn| turn.state == TurnState::Running)
            {
                ThreadStatus::Running
            } else if state
                .inputs
                .iter()
                .any(|input| input.state == InputState::Pending)
            {
                ThreadStatus::Queued
            } else {
                ThreadStatus::Idle
            }
        }
    }
}

pub(in crate::studio) fn saved_mode(
    state: &ThreadSnapshot,
) -> Result<Option<pl_protocol::ThreadModeId>, ProjectionError> {
    state
        .extensions
        .get("studio.mode")
        .map(|record| {
            if record.payload.format() != "pl.studio.mode" || record.payload.version() != 1 {
                return Err(ProjectionError::UnsupportedOutput(
                    "unsupported saved Mode".into(),
                ));
            }
            Ok(serde_json::from_str(record.payload.content())?)
        })
        .transpose()
}

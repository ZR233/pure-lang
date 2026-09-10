//! Thread Mode 状态图的轻量持久状态生命周期。

pub const MAX_WORKFLOW_STATE_BYTES: usize = 256 * 1024;

pub const MAX_WORKFLOW_HISTORY: usize = 64;

pub const MAX_ARCHIVED_WORKFLOW_RUNS: usize = 16;

pub const MAX_WORKFLOW_OPERATION_RECEIPTS: usize = 32;

use pl_protocol::{
    PureError, ThreadModeId, WorkflowOperationReceipt, WorkflowRun, WorkflowRunArchive,
    WorkflowRunLifecycle, WorkflowSessionState, WorkflowStateKind,
};

/// Archives an incompatible run when an idle root Thread changes Mode.
///
/// This operation deliberately does not create the new Mode's initial run. The next root Turn
/// creates it immediately before the first provider request.
pub fn archive_workflow_for_mode_change(
    existing: Option<WorkflowSessionState>,
    next_mode_id: &ThreadModeId,
    now: i64,
) -> Result<Option<WorkflowSessionState>, PureError> {
    let Some(mut state) = existing else {
        return Ok(None);
    };
    let mode_changed = state
        .current_run
        .as_ref()
        .is_some_and(|run| &run.mode_id != next_mode_id);
    if mode_changed {
        let run = state
            .current_run
            .take()
            .expect("mode change was derived from a current run");
        archive_run(
            &mut state,
            run,
            "modeChanged",
            "Thread Mode changed while idle",
            now,
        )?;
        state.revision = state.revision.saturating_add(1);
    }
    validate_session_state_size(&state)?;
    Ok(Some(state))
}

/// Archives a completed or replaced run, maintaining the bounded archive digest.
///
/// # Errors
/// Returns serialization errors while updating archived history.
pub fn archive_run(
    state: &mut WorkflowSessionState,
    run: WorkflowRun,
    outcome: &str,
    summary: &str,
    now: i64,
) -> Result<(), PureError> {
    state.archived_runs.push(WorkflowRunArchive {
        lineage_id: run.lineage_id,
        run_id: run.run_id,
        mode_id: run.mode_id,
        graph_revision: run.graph_revision,
        graph_hash: run.graph_hash,
        final_state_id: run.current_state_id,
        outcome: outcome.to_string(),
        summary: summary.to_string(),
        archived_at: now,
    });
    while state.archived_runs.len() > MAX_ARCHIVED_WORKFLOW_RUNS {
        let archived = state.archived_runs.remove(0);
        let encoded = serde_json::to_vec(&archived)?;
        state.archived_run_digest = crate::canonical_content_hash(
            [state.archived_run_digest.as_bytes(), encoded.as_slice()]
                .concat()
                .as_slice(),
        );
        state.archived_run_count = state.archived_run_count.saturating_add(1);
    }
    Ok(())
}

/// Moves excess transition records into the archive digest.
///
/// # Errors
/// Returns serialization errors while updating archived history.
pub fn trim_history(run: &mut WorkflowRun) -> Result<(), PureError> {
    let drain = run.history_tail.len().saturating_sub(MAX_WORKFLOW_HISTORY);
    for record in run.history_tail.drain(..drain) {
        let encoded = serde_json::to_vec(&record)?;
        run.archived_transition_digest = crate::canonical_content_hash(
            [
                run.archived_transition_digest.as_bytes(),
                encoded.as_slice(),
            ]
            .concat()
            .as_slice(),
        );
        run.archived_transition_count = run.archived_transition_count.saturating_add(1);
    }
    Ok(())
}

/// Records the accepted operation revision and bounds the idempotency receipt history.
pub fn commit_operation_receipt(
    state: &mut WorkflowSessionState,
    operation_id: String,
    argument_hash: String,
    revision: u64,
) {
    state.revision = revision;
    state.operation_receipts.push(WorkflowOperationReceipt {
        operation_id,
        argument_hash,
        operation_revision: revision,
    });
    let drain = state
        .operation_receipts
        .len()
        .saturating_sub(MAX_WORKFLOW_OPERATION_RECEIPTS);
    state.operation_receipts.drain(..drain);
}

/// Finds an existing receipt by its stable operation identity.
pub fn operation_receipt<'a>(
    state: &'a WorkflowSessionState,
    operation_id: &str,
) -> Option<&'a WorkflowOperationReceipt> {
    state
        .operation_receipts
        .iter()
        .find(|receipt| receipt.operation_id == operation_id)
}

/// Projects a graph state kind into the corresponding run lifecycle.
pub fn lifecycle_for_state(kind: WorkflowStateKind) -> WorkflowRunLifecycle {
    match kind {
        WorkflowStateKind::Atomic => WorkflowRunLifecycle::Active,
        WorkflowStateKind::Final => WorkflowRunLifecycle::Terminal,
    }
}

pub fn validate_session_state_size(state: &WorkflowSessionState) -> Result<(), PureError> {
    let bytes = serde_json::to_vec(state)?;
    if bytes.len() > MAX_WORKFLOW_STATE_BYTES {
        return Err(PureError::ConfigError(format!(
            "workflow state exceeds {MAX_WORKFLOW_STATE_BYTES} bytes"
        )));
    }
    Ok(())
}

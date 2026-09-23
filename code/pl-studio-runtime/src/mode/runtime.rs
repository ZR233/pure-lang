//! Studio mode selection and initial workflow creation.
use super::{CompiledWorkflowDefinition, RegisteredThreadMode};
use crate::mode::state::{archive_run, lifecycle_for_state, validate_session_state_size};
use pl_protocol::{PureError, WorkflowRun, WorkflowRunLifecycle, WorkflowSessionState};
/// 在 root Turn 第一次 provider 请求前，把持久状态与冻结 Mode 快照对齐。
pub fn reconcile_workflow_for_turn(
    existing: Option<WorkflowSessionState>,
    mode: &RegisteredThreadMode,
    identity_seed: &str,
    now: i64,
) -> Result<Option<WorkflowSessionState>, PureError> {
    let mut state = existing.unwrap_or_default();
    let Some(graph) = mode.workflow() else {
        if let Some(run) = state.current_run.take() {
            archive_run(
                &mut state,
                run,
                "modeChanged",
                "Selected Mode has no workflow",
                now,
            )?;
            state.revision = state.revision.saturating_add(1);
        }
        validate_session_state_size(&state)?;
        return if is_empty_state(&state) {
            Ok(None)
        } else {
            Ok(Some(state))
        };
    };

    let replacement = match state.current_run.as_ref() {
        None => Some((None, "started")),
        Some(run) if run.lifecycle == WorkflowRunLifecycle::Terminal => {
            Some((None, "terminalRestart"))
        }
        Some(run) if run.mode_id != mode.descriptor().id => Some((None, "modeChanged")),
        Some(run) if run.graph_hash != graph.graph_hash() => {
            Some((Some(run.lineage_id.clone()), "modeUpdated"))
        }
        Some(_) => None,
    };
    if let Some((replacement_lineage, reason)) = replacement {
        if let Some(run) = state.current_run.take() {
            archive_run(&mut state, run, reason, "Started replacement run", now)?;
        }
        let revision = state.revision.saturating_add(1);
        let seed = format!("{identity_seed}:{reason}:{revision}");
        state.current_run = Some(new_run(mode, graph, replacement_lineage, &seed, now));
        state.revision = revision;
    }
    validate_session_state_size(&state)?;
    Ok(Some(state))
}
/// Creates an initial run from an already validated, frozen mode graph.
pub fn new_run(
    mode: &RegisteredThreadMode,
    graph: &CompiledWorkflowDefinition,
    lineage_id: Option<String>,
    identity_seed: &str,
    now: i64,
) -> WorkflowRun {
    let initial = graph.initial_state();
    WorkflowRun {
        lineage_id: lineage_id.unwrap_or_else(|| generated_id("lineage", identity_seed)),
        run_id: generated_id("run", identity_seed),
        mode_id: mode.descriptor().id.clone(),
        graph_revision: mode.graph_revision(),
        graph_hash: graph.graph_hash().to_string(),
        lifecycle: lifecycle_for_state(initial.kind),
        current_state_id: initial.id.clone(),
        started_at: now,
        updated_at: now,
        history_tail: Vec::new(),
        archived_transition_count: 0,
        archived_transition_digest: String::new(),
    }
}

pub(crate) fn generated_id(prefix: &str, seed: &str) -> String {
    let hash = pl_core::context::content_hash(seed.as_bytes());
    format!("{prefix}-{}", &hash[7..31])
}

fn is_empty_state(state: &WorkflowSessionState) -> bool {
    state.revision == 0
        && state.current_run.is_none()
        && state.archived_runs.is_empty()
        && state.archived_run_count == 0
        && state.operation_receipts.is_empty()
}

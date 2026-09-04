//! Thread Mode 状态图的轻量持久状态生命周期。

use pl_protocol::{
    PureError, ThreadModeId, WorkflowOperationReceipt, WorkflowRun, WorkflowRunArchive,
    WorkflowRunLifecycle, WorkflowSessionState, WorkflowStateKind,
};

use super::{
    CompiledWorkflowDefinition, MAX_ARCHIVED_WORKFLOW_RUNS, MAX_WORKFLOW_HISTORY,
    MAX_WORKFLOW_OPERATION_RECEIPTS, MAX_WORKFLOW_STATE_BYTES, RegisteredThreadMode,
};

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

pub(crate) fn new_run(
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

pub(crate) fn archive_run(
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

pub(crate) fn trim_history(run: &mut WorkflowRun) -> Result<(), PureError> {
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

pub(crate) fn commit_operation_receipt(
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

pub(crate) fn operation_receipt<'a>(
    state: &'a WorkflowSessionState,
    operation_id: &str,
) -> Option<&'a WorkflowOperationReceipt> {
    state
        .operation_receipts
        .iter()
        .find(|receipt| receipt.operation_id == operation_id)
}

pub(crate) fn lifecycle_for_state(kind: WorkflowStateKind) -> WorkflowRunLifecycle {
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

pub(crate) fn generated_id(prefix: &str, seed: &str) -> String {
    let hash = crate::canonical_content_hash(seed.as_bytes());
    format!("{prefix}-{}", &hash[7..31])
}

fn is_empty_state(state: &WorkflowSessionState) -> bool {
    state.revision == 0
        && state.current_run.is_none()
        && state.archived_runs.is_empty()
        && state.archived_run_count == 0
        && state.operation_receipts.is_empty()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::thread::test_support::{graph, registered, registration, source};
    use crate::thread::{ThreadModeManager, ThreadModeSourceKind};
    use pl_protocol::ThreadModeId;

    #[test]
    fn first_turn_starts_the_registered_initial_state() {
        let manager = ThreadModeManager::default();
        let mode = registered(&manager, "upstream.start", "Prompt", graph(""));

        let state = reconcile_workflow_for_turn(None, &mode, "thread-1", 10)
            .expect("reconcile succeeds")
            .expect("workflow state");
        let run = state.current_run.expect("current run");

        assert_eq!(run.current_state_id, "ready");
        assert_eq!(run.graph_revision, mode.graph_revision());
        assert_eq!(run.lifecycle, WorkflowRunLifecycle::Active);
    }

    #[test]
    fn prompt_only_update_keeps_the_existing_run() {
        let manager = ThreadModeManager::default();
        let first = registered(&manager, "upstream.runtime-prompt", "One", graph(""));
        let state = reconcile_workflow_for_turn(None, &first, "thread-1", 10)
            .expect("first reconcile")
            .expect("workflow state");
        let run_id = state.current_run.as_ref().expect("run").run_id.clone();
        let revision = state.revision;
        let second = registered(&manager, "upstream.runtime-prompt", "Two", graph(""));

        let reconciled = reconcile_workflow_for_turn(Some(state), &second, "thread-1", 20)
            .expect("second reconcile")
            .expect("workflow state");

        assert_eq!(reconciled.revision, revision);
        assert_eq!(reconciled.current_run.expect("run").run_id, run_id);
    }

    #[test]
    fn graph_update_archives_and_replaces_within_the_same_lineage() {
        let manager = ThreadModeManager::default();
        let first = registered(&manager, "upstream.runtime-graph", "Prompt", graph(""));
        let state = reconcile_workflow_for_turn(None, &first, "thread-1", 10)
            .expect("first reconcile")
            .expect("workflow state");
        let old = state.current_run.as_ref().expect("run").clone();
        let second = registered(
            &manager,
            "upstream.runtime-graph",
            "Prompt",
            graph(" again"),
        );

        let reconciled = reconcile_workflow_for_turn(Some(state), &second, "thread-1", 20)
            .expect("second reconcile")
            .expect("workflow state");
        let replacement = reconciled.current_run.as_ref().expect("replacement");

        assert_eq!(replacement.lineage_id, old.lineage_id);
        assert_ne!(replacement.run_id, old.run_id);
        assert_eq!(
            reconciled.archived_runs.last().expect("archive").outcome,
            "modeUpdated"
        );
    }

    #[test]
    fn terminal_run_restart_creates_a_new_lineage() {
        let manager = ThreadModeManager::default();
        let mode = registered(&manager, "upstream.terminal", "Prompt", graph(""));
        let mut state = reconcile_workflow_for_turn(None, &mode, "thread-1", 10)
            .expect("first reconcile")
            .expect("workflow state");
        let old_lineage = state.current_run.as_ref().expect("run").lineage_id.clone();
        state.current_run.as_mut().expect("run").lifecycle = WorkflowRunLifecycle::Terminal;

        let restarted = reconcile_workflow_for_turn(Some(state), &mode, "thread-1", 20)
            .expect("restart reconcile")
            .expect("workflow state");

        assert_ne!(
            restarted.current_run.expect("replacement").lineage_id,
            old_lineage
        );
        assert_eq!(
            restarted.archived_runs.last().expect("archive").outcome,
            "terminalRestart"
        );
    }

    #[test]
    fn selecting_a_mode_without_a_graph_archives_the_current_run() {
        let manager = ThreadModeManager::default();
        let workflow_mode = registered(&manager, "upstream.mode-change", "Prompt", graph(""));
        let state = reconcile_workflow_for_turn(None, &workflow_mode, "thread-1", 10)
            .expect("first reconcile")
            .expect("workflow state");
        let simple_id = ThreadModeId::new("mode.simple-test").expect("valid mode id");
        let simple = manager
            .replace_source(
                source("upstream.simple", ThreadModeSourceKind::External),
                [registration(simple_id.as_str(), "Simple", None)],
            )
            .expect("simple registration")
            .mode(&simple_id)
            .expect("simple mode");

        let reconciled = reconcile_workflow_for_turn(Some(state), &simple, "thread-1", 20)
            .expect("mode change")
            .expect("archive-only state");

        assert!(reconciled.current_run.is_none());
        assert_eq!(
            reconciled.archived_runs.last().expect("archive").outcome,
            "modeChanged"
        );
    }

    #[test]
    fn changing_mode_while_idle_archives_without_starting_the_replacement() {
        let manager = ThreadModeManager::default();
        let mode = registered(&manager, "upstream.idle-change", "Prompt", graph(""));
        let state = reconcile_workflow_for_turn(None, &mode, "thread-1", 10)
            .expect("first reconcile")
            .expect("workflow state");
        let next_mode = ThreadModeId::new("mode.other").expect("valid mode id");

        let changed = archive_workflow_for_mode_change(Some(state), &next_mode, 20)
            .expect("archive mode change")
            .expect("archive state");

        assert!(changed.current_run.is_none());
        assert_eq!(
            changed.archived_runs.last().expect("archive").outcome,
            "modeChanged"
        );
        assert_eq!(
            archive_workflow_for_mode_change(None, &next_mode, 20).expect("empty state"),
            None
        );
    }
}

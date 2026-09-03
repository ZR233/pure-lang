use pretty_assertions::assert_eq;

use super::*;
use pl_protocol::{
    ThreadModeId, WorkflowDefinition, WorkflowRunLifecycle, WorkflowState, WorkflowStateKind,
    WorkflowTransition,
};

const STATIC_STATES: &[StaticWorkflowState] = &[
    StaticWorkflowState {
        id: "ready",
        title: "Ready",
        instructions: "Prepare the delivery.",
        completion_criteria: &["Delivery is prepared."],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "done",
        title: "Done",
        instructions: "",
        completion_criteria: &[],
        kind: WorkflowStateKind::Final,
    },
];
const STATIC_TRANSITIONS: &[StaticWorkflowTransition] = &[StaticWorkflowTransition {
    source_state_id: "ready",
    target_state_id: "done",
    guard: "Delivery is verified.",
}];
const STATIC_REGISTRATION: StaticThreadModeRegistration = StaticThreadModeRegistration {
    id: "mode.static-test",
    display_name: "Static test",
    description: "A built-in static descriptor",
    order: 30,
    prompt: "Follow the registered graph.",
    workflow: Some(StaticWorkflowDefinition {
        title: "Delivery",
        goal: "Ship a verified delivery",
        initial_state_id: "ready",
        states: STATIC_STATES,
        transitions: STATIC_TRANSITIONS,
    }),
};

fn source(id: &str, kind: ThreadModeSourceKind) -> ThreadModeSource {
    ThreadModeSource {
        id: ThreadModeSourceId::new(id).expect("valid source id"),
        kind,
    }
}

fn graph(extra_guard: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        title: "Delivery".to_string(),
        goal: "Ship a verified delivery".to_string(),
        initial_state_id: "ready".to_string(),
        states: vec![
            WorkflowState {
                id: "ready".to_string(),
                title: "Ready".to_string(),
                instructions: "Prepare the delivery.".to_string(),
                completion_criteria: vec!["Delivery is prepared.".to_string()],
                kind: WorkflowStateKind::Atomic,
            },
            WorkflowState {
                id: "done".to_string(),
                title: "Done".to_string(),
                instructions: String::new(),
                completion_criteria: Vec::new(),
                kind: WorkflowStateKind::Final,
            },
        ],
        transitions: vec![WorkflowTransition {
            source_state_id: "ready".to_string(),
            target_state_id: "done".to_string(),
            guard: format!("Delivery is verified{extra_guard}."),
        }],
    }
}

fn registration(
    id: &str,
    prompt: &str,
    workflow: Option<WorkflowDefinition>,
) -> ThreadModeRegistration {
    ThreadModeRegistration {
        id: ThreadModeId::new(id).expect("valid mode id"),
        display_name: id.to_string(),
        description: "Synthetic in-memory registration".to_string(),
        order: 20,
        prompt: prompt.to_string(),
        workflow,
    }
}

fn registered(
    manager: &ThreadModeManager,
    source_id: &str,
    prompt: &str,
    definition: WorkflowDefinition,
) -> std::sync::Arc<RegisteredThreadMode> {
    let id = ThreadModeId::new("mode.synthetic").expect("valid mode id");
    manager
        .replace_source(
            source(source_id, ThreadModeSourceKind::External),
            [registration(id.as_str(), prompt, Some(definition))],
        )
        .expect("registration succeeds")
        .mode(&id)
        .expect("registered mode")
}

#[test]
fn static_registration_converts_to_the_public_owned_input() {
    let converted = STATIC_REGISTRATION
        .to_registration()
        .expect("static registration is valid");

    assert_eq!(converted.id.as_str(), "mode.static-test");
    assert_eq!(converted.prompt, "Follow the registered graph.");
    assert_eq!(converted.workflow, Some(graph("")));
}

#[test]
fn synthetic_upstream_registration_uses_only_the_public_memory_api() {
    let manager = ThreadModeManager::default();
    let mode = registered(&manager, "upstream.test", "External prompt", graph(""));

    assert_eq!(mode.prompt(), "External prompt");
    assert_eq!(mode.descriptor().id.as_str(), "mode.synthetic");
    assert!(mode.workflow().is_some());
    assert_eq!(manager.snapshot().catalog().modes.len(), 1);
}

#[test]
fn source_replacement_is_atomic_when_one_registration_is_invalid() {
    let manager = ThreadModeManager::default();
    let initial = registered(&manager, "upstream.atomic", "Initial", graph(""));
    let before = manager.snapshot();
    let mut invalid = graph("");
    invalid.initial_state_id = "missing".to_string();

    let result = manager.replace_source(
        source("upstream.atomic", ThreadModeSourceKind::External),
        [
            registration("mode.synthetic", "Replacement", Some(graph(""))),
            registration("mode.invalid", "Invalid", Some(invalid)),
        ],
    );

    assert!(matches!(
        result,
        Err(ThreadModeManagerError::InvalidWorkflow { .. })
    ));
    let after = manager.snapshot();
    assert_eq!(after.revision(), before.revision());
    assert_eq!(
        after
            .mode(&initial.descriptor().id)
            .expect("old mode remains")
            .prompt(),
        "Initial"
    );
}

#[test]
fn prompt_only_replacement_preserves_graph_identity() {
    let manager = ThreadModeManager::default();
    let first = registered(&manager, "upstream.prompt", "Prompt one", graph(""));
    let first_revision = first.graph_revision();
    let first_hash = first.graph_hash().expect("graph hash").to_string();
    let second = registered(&manager, "upstream.prompt", "Prompt two", graph(""));

    assert_eq!(second.prompt(), "Prompt two");
    assert_eq!(second.graph_revision(), first_revision);
    assert_eq!(second.graph_hash(), Some(first_hash.as_str()));
}

#[test]
fn graph_replacement_allocates_a_new_revision_and_hash() {
    let manager = ThreadModeManager::default();
    let first = registered(&manager, "upstream.graph", "Prompt", graph(""));
    let second = registered(&manager, "upstream.graph", "Prompt", graph(" again"));

    assert!(second.graph_revision() > first.graph_revision());
    assert_ne!(second.graph_hash(), first.graph_hash());
}

#[test]
fn another_source_cannot_override_a_builtin_mode() {
    let manager = ThreadModeManager::default();
    manager
        .replace_source(
            source("studio.builtin", ThreadModeSourceKind::Builtin),
            [registration("mode.synthetic", "Builtin", Some(graph("")))],
        )
        .expect("builtin registration");

    let result = manager.replace_source(
        source("upstream.override", ThreadModeSourceKind::External),
        [registration("mode.synthetic", "Override", Some(graph("")))],
    );

    assert!(matches!(
        result,
        Err(ThreadModeManagerError::SourceConflict { .. })
    ));
}

#[test]
fn source_identity_cannot_change_from_builtin_to_external() {
    let manager = ThreadModeManager::default();
    manager
        .replace_source(
            source("studio.builtin", ThreadModeSourceKind::Builtin),
            [registration("mode.synthetic", "Builtin", Some(graph("")))],
        )
        .expect("builtin registration");

    let result = manager.replace_source(
        source("studio.builtin", ThreadModeSourceKind::External),
        [registration("mode.synthetic", "Override", Some(graph("")))],
    );

    assert!(matches!(
        result,
        Err(ThreadModeManagerError::SourceKindConflict { .. })
    ));
    assert_eq!(
        manager
            .snapshot()
            .mode(&ThreadModeId::new("mode.synthetic").expect("valid mode id"))
            .expect("builtin remains")
            .prompt(),
        "Builtin"
    );
}

#[test]
fn removal_publishes_a_new_catalog_without_the_source() {
    let manager = ThreadModeManager::default();
    registered(&manager, "upstream.removed", "Prompt", graph(""));
    let before = manager.snapshot().revision();

    let snapshot = manager
        .remove_source(&ThreadModeSourceId::new("upstream.removed").expect("valid source id"));

    assert!(snapshot.revision() > before);
    assert!(snapshot.catalog().modes.is_empty());
}

#[test]
fn normalized_graph_order_has_a_stable_hash() {
    let first = compile_workflow_definition(graph("")).expect("valid graph");
    let mut reordered = graph("");
    reordered.states.reverse();
    let second = compile_workflow_definition(reordered).expect("valid reordered graph");

    assert_eq!(first.graph_hash(), second.graph_hash());
}

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

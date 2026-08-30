use pl_protocol::{
    ModeInstructionSnapshot, WorkflowDefinition, WorkflowRun, WorkflowRunLifecycle,
    WorkflowSessionState, WorkflowStage, WorkflowTransition,
};
use pretty_assertions::assert_eq;

use super::*;

fn stage(id: &str, terminal: bool) -> WorkflowStage {
    WorkflowStage {
        id: id.to_string(),
        title: id.to_string(),
        instructions: if !terminal {
            format!("Complete {id}")
        } else {
            String::new()
        },
        completion_criteria: if !terminal {
            vec![format!("{id} is complete")]
        } else {
            Vec::new()
        },
        terminal,
    }
}

fn valid_definition() -> WorkflowDefinition {
    WorkflowDefinition {
        title: "Task".to_string(),
        goal: "Finish the task".to_string(),
        initial_stage_id: "planning".to_string(),
        stages: vec![stage("done", true), stage("planning", false)],
        transitions: vec![WorkflowTransition {
            from_stage_id: "planning".to_string(),
            to_stage_id: "done".to_string(),
            when: "Planning is complete".to_string(),
        }],
    }
}

#[test]
fn compiler_normalizes_whitespace_and_produces_a_stable_hash() {
    let first = compile_definition(valid_definition()).unwrap();
    let mut equivalent = valid_definition();
    equivalent.title = "  Task  ".to_string();
    equivalent.stages[0].title = "  done  ".to_string();
    let second = compile_definition(equivalent).unwrap();

    assert_eq!(first.definition, second.definition);
    assert_eq!(first.definition_hash, second.definition_hash);
}

#[test]
fn compiler_reports_duplicate_ids_and_unknown_endpoints() {
    let mut definition = valid_definition();
    definition.stages.push(stage("planning", false));
    definition.transitions.push(WorkflowTransition {
        from_stage_id: "missing".to_string(),
        to_stage_id: "done".to_string(),
        when: "Never".to_string(),
    });

    let error = compile_definition(definition).unwrap_err();
    let codes = error
        .issues()
        .iter()
        .map(|issue| issue.code)
        .collect::<Vec<_>>();
    assert!(codes.contains(&"duplicateStageId"));
    assert!(codes.contains(&"unknownEndpoint"));
}

#[test]
fn compiler_accepts_cycles_when_every_stage_can_reach_a_terminal() {
    let mut definition = valid_definition();
    definition.stages.insert(1, stage("working", false));
    definition.transitions = vec![
        WorkflowTransition {
            from_stage_id: "planning".to_string(),
            to_stage_id: "working".to_string(),
            when: "Ready".to_string(),
        },
        WorkflowTransition {
            from_stage_id: "working".to_string(),
            to_stage_id: "planning".to_string(),
            when: "Plan needs revision".to_string(),
        },
        WorkflowTransition {
            from_stage_id: "working".to_string(),
            to_stage_id: "done".to_string(),
            when: "Implementation is complete".to_string(),
        },
    ];

    compile_definition(definition).unwrap();
}

#[test]
fn compiler_rejects_an_unreachable_stage_and_a_closed_cycle() {
    let mut definition = valid_definition();
    definition.stages.push(stage("loop-a", false));
    definition.stages.push(stage("loop-b", false));
    definition.transitions.extend([
        WorkflowTransition {
            from_stage_id: "loop-a".to_string(),
            to_stage_id: "loop-b".to_string(),
            when: "Continue".to_string(),
        },
        WorkflowTransition {
            from_stage_id: "loop-b".to_string(),
            to_stage_id: "loop-a".to_string(),
            when: "Continue".to_string(),
        },
    ]);

    let error = compile_definition(definition).unwrap_err();
    assert!(
        error
            .issues()
            .iter()
            .any(|issue| issue.code == "unreachableStage")
    );
    assert!(
        error
            .issues()
            .iter()
            .any(|issue| issue.code == "cannotReachTerminal")
    );
}

#[test]
fn compiler_requires_a_terminal_and_accepts_multiple_terminals() {
    let mut without_terminal = valid_definition();
    without_terminal.stages[0] = stage("done", false);
    let error = compile_definition(without_terminal).unwrap_err();
    assert!(
        error
            .issues()
            .iter()
            .any(|issue| issue.code == "missingTerminalStage")
    );

    let mut multiple_terminals = valid_definition();
    multiple_terminals.stages.push(stage("stopped", true));
    multiple_terminals.transitions.push(WorkflowTransition {
        from_stage_id: "planning".to_string(),
        to_stage_id: "stopped".to_string(),
        when: "Work cannot continue".to_string(),
    });
    compile_definition(multiple_terminals).unwrap();
}

#[test]
fn compiler_rejects_duplicate_edges() {
    let mut definition = valid_definition();
    definition
        .transitions
        .push(definition.transitions[0].clone());

    let error = compile_definition(definition).unwrap_err();
    assert!(
        error
            .issues()
            .iter()
            .any(|issue| issue.code == "duplicateTransition")
    );
}

#[test]
fn projection_contains_only_current_constraints_and_allowed_edges() {
    let compiled = compile_definition(valid_definition()).unwrap();
    let state = WorkflowSessionState {
        revision: 4,
        current_run: Some(WorkflowRun {
            lineage_id: "lineage-1".to_string(),
            run_id: "run-1".to_string(),
            definition: compiled.definition,
            definition_hash: compiled.definition_hash,
            mode: ModeInstructionSnapshot {
                mode_id: "mode.task".to_string(),
                ..ModeInstructionSnapshot::default()
            },
            lifecycle: WorkflowRunLifecycle::Active,
            current_stage_id: "planning".to_string(),
            compiled_at: 1,
            updated_at: 1,
            history_tail: Vec::new(),
            archived_transition_count: 0,
            archived_transition_digest: String::new(),
        }),
        ..WorkflowSessionState::default()
    };

    let section = model_context_section(&state).unwrap();
    assert_eq!(section.id.as_str(), WORKFLOW_CONTEXT_SECTION_ID);
    assert!(section.content.contains("planning"));
    assert!(section.content.contains("done"));
    assert!(!section.content.contains("mode.task Skill"));
}

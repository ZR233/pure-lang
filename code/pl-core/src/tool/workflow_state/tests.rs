use pl_protocol::{ModeInstructionSnapshot, WorkflowRunLifecycle};
use pretty_assertions::assert_eq;

use super::*;

fn tool() -> WorkflowStateTool {
    WorkflowStateTool::new(
        crate::TurnWorkingSetHandle::default(),
        ModeInstructionSnapshot {
            mode_id: "mode.task".to_string(),
            display_name: "Task".to_string(),
            content_hash: "sha256:mode".to_string(),
            content: "Task mode".to_string(),
            ..ModeInstructionSnapshot::default()
        },
    )
}

fn context(turn_id: &str, call_id: &str) -> ToolCallContext {
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    ToolCallContext::new(
        crate::ToolCallIdentity {
            turn_id: turn_id.to_string(),
            call_id: call_id.to_string(),
            ..crate::ToolCallIdentity::default()
        },
        event_tx,
    )
}

fn definition() -> WorkflowDefinitionInput {
    WorkflowDefinitionInput {
        title: "Task".to_string(),
        goal: "Complete task".to_string(),
        initial_stage_id: "working".to_string(),
        stages: vec![
            WorkflowStageInput {
                id: "working".to_string(),
                title: "Working".to_string(),
                instructions: "Do the work".to_string(),
                completion_criteria: vec!["Work is verified".to_string()],
                terminal: false,
            },
            WorkflowStageInput {
                id: "completed".to_string(),
                title: "Completed".to_string(),
                instructions: String::new(),
                completion_criteria: Vec::new(),
                terminal: true,
            },
        ],
        transitions: vec![WorkflowTransitionInput {
            from_stage_id: "working".to_string(),
            to_stage_id: "completed".to_string(),
            when: "Work is verified".to_string(),
        }],
    }
}

fn indirect_definition() -> WorkflowDefinitionInput {
    WorkflowDefinitionInput {
        title: "Indirect task".to_string(),
        goal: "Review before completion".to_string(),
        initial_stage_id: "working".to_string(),
        stages: vec![
            WorkflowStageInput {
                id: "working".to_string(),
                title: "Working".to_string(),
                instructions: "Do the work".to_string(),
                completion_criteria: vec!["Implementation is ready".to_string()],
                terminal: false,
            },
            WorkflowStageInput {
                id: "reviewing".to_string(),
                title: "Reviewing".to_string(),
                instructions: "Review the work".to_string(),
                completion_criteria: vec!["Review passes".to_string()],
                terminal: false,
            },
            WorkflowStageInput {
                id: "completed".to_string(),
                title: "Completed".to_string(),
                instructions: String::new(),
                completion_criteria: Vec::new(),
                terminal: true,
            },
        ],
        transitions: vec![
            WorkflowTransitionInput {
                from_stage_id: "working".to_string(),
                to_stage_id: "reviewing".to_string(),
                when: "Implementation is ready".to_string(),
            },
            WorkflowTransitionInput {
                from_stage_id: "reviewing".to_string(),
                to_stage_id: "completed".to_string(),
                when: "Review passes".to_string(),
            },
        ],
    }
}

fn transition_input(
    run_id: impl Into<String>,
    revision: u64,
    stage_id: impl Into<String>,
    target_id: impl Into<String>,
) -> WorkflowStateInput {
    WorkflowStateInput::Transition {
        expected_run_id: run_id.into(),
        expected_revision: revision,
        expected_stage_id: stage_id.into(),
        to_stage_id: target_id.into(),
        reason: "Current stage is complete".to_string(),
        completion: WorkflowCompletionInput {
            summary: "Stage criteria are satisfied".to_string(),
            evidence: vec!["verified".to_string()],
        },
    }
}

#[test]
fn workflow_state_wire_contract_uses_camel_case_variant_fields() {
    let schema = tool().input_schema();
    let variants = schema["oneOf"]
        .as_array()
        .expect("workflow_state schema must expose tagged variants");
    let compile = variants
        .iter()
        .find(|variant| {
            variant
                .pointer("/properties/action/const")
                .and_then(serde_json::Value::as_str)
                == Some("compile")
        })
        .expect("compile schema variant");
    let properties = compile["properties"]
        .as_object()
        .expect("compile properties");
    assert!(properties.contains_key("expectedRevision"));
    assert!(properties.contains_key("expectedRunId"));
    assert!(!properties.contains_key("expected_revision"));
    assert!(!properties.contains_key("expected_run_id"));
    assert_eq!(
        compile["required"],
        serde_json::json!(["action", "expectedRevision", "definition"])
    );

    let input = serde_json::from_value::<WorkflowStateInput>(serde_json::json!({
        "action": "compile",
        "expectedRevision": 0,
        "definition": {
            "title": "Task",
            "goal": "Complete task",
            "initialStageId": "working",
            "stages": [
                {"id":"working","title":"Working","instructions":"Do the work","completionCriteria":["Work is verified"]},
                {"id":"completed","title":"Completed","instructions":"","completionCriteria":[],"terminal":true}
            ],
            "transitions": [{"fromStageId":"working","toStageId":"completed","when":"Work is verified"}]
        }
    }))
    .expect("camelCase workflow_state arguments must deserialize");
    assert!(matches!(
        input,
        WorkflowStateInput::Compile {
            expected_revision: 0,
            expected_run_id: None,
            ..
        }
    ));
}

#[test]
fn wire_validation_allows_only_a_partial_transition_without_summary() {
    let arguments = serde_json::json!({
        "action": "transition",
        "expectedRunId": "run-1",
        "expectedRevision": 1,
        "expectedStageId": "planning",
        "toStageId": "awaiting_confirmation",
        "reason": "Plan is ready",
        "completion": {
            "evidence": ["plan is complete"]
        }
    });

    assert!(validate_workflow_state_arguments(arguments.clone()).is_err());
    assert!(validate_workflow_state_wire_arguments(arguments).is_ok());
}

#[test]
fn compile_transition_and_replay_use_canonical_cas() {
    let tool = tool();
    let compile_context = context("turn-1", "call-1");
    let compile_arguments = serde_json::json!({
        "action": "compile",
        "expectedRevision": 0,
        "expectedRunId": null,
        "definition": {
            "title": "Task",
            "goal": "Complete task",
            "initialStageId": "working",
            "stages": [
                {"id":"working","title":"Working","instructions":"Do the work","completionCriteria":["Work is verified"]},
                {"id":"completed","title":"Completed","instructions":"","completionCriteria":[],"terminal":true}
            ],
            "transitions": [{"fromStageId":"working","toStageId":"completed","when":"Work is verified"}]
        }
    });
    let compile_hash = crate::canonical_json_hash(&compile_arguments);
    let first = tool
        .execute_action(
            WorkflowStateInput::Compile {
                expected_revision: 0,
                expected_run_id: None,
                definition: definition(),
            },
            &compile_context,
            compile_hash.clone(),
        )
        .unwrap();
    assert!(first.accepted);
    assert_eq!(first.code, "compiled");

    let replay = tool
        .execute_action(
            WorkflowStateInput::Compile {
                expected_revision: 0,
                expected_run_id: None,
                definition: definition(),
            },
            &compile_context,
            compile_hash,
        )
        .unwrap();
    assert_eq!(replay.code, "alreadyApplied");

    let state = tool.working_set.workflow().unwrap();
    let run_id = state.current_run.as_ref().unwrap().run_id.clone();
    let transitioned = tool
        .execute_action(
            WorkflowStateInput::Transition {
                expected_run_id: run_id,
                expected_revision: 1,
                expected_stage_id: "working".to_string(),
                to_stage_id: "completed".to_string(),
                reason: "Verified".to_string(),
                completion: WorkflowCompletionInput {
                    summary: "All work passed".to_string(),
                    evidence: vec!["cargo test".to_string()],
                },
            },
            &context("turn-2", "call-2"),
            "sha256:transition".to_string(),
        )
        .unwrap();
    assert_eq!(transitioned.code, "transitioned");
    let run = tool.working_set.workflow().unwrap().current_run.unwrap();
    assert_eq!(run.lifecycle, WorkflowRunLifecycle::Terminal);
    assert_eq!(run.current_stage_id, "completed");
}

#[test]
fn stale_transition_is_rejected_without_mutation() {
    let tool = tool();
    let _ = tool
        .execute_action(
            WorkflowStateInput::Compile {
                expected_revision: 0,
                expected_run_id: None,
                definition: definition(),
            },
            &context("turn-1", "call-1"),
            "sha256:compile".to_string(),
        )
        .unwrap();
    let run_id = tool
        .working_set
        .workflow()
        .unwrap()
        .current_run
        .unwrap()
        .run_id;
    let rejected = tool
        .execute_action(
            WorkflowStateInput::Transition {
                expected_run_id: run_id,
                expected_revision: 0,
                expected_stage_id: "working".to_string(),
                to_stage_id: "completed".to_string(),
                reason: "Done".to_string(),
                completion: WorkflowCompletionInput {
                    summary: "Done".to_string(),
                    evidence: Vec::new(),
                },
            },
            &context("turn-2", "call-2"),
            "sha256:transition".to_string(),
        )
        .unwrap();

    assert!(!rejected.accepted);
    assert_eq!(rejected.code, "staleRevision");
    assert_eq!(tool.working_set.workflow().unwrap().revision, 1);
}

#[test]
fn supersede_compiles_before_replacing_and_keeps_lineage_and_mode() {
    let tool = tool();
    let _ = tool
        .execute_action(
            WorkflowStateInput::Compile {
                expected_revision: 0,
                expected_run_id: None,
                definition: definition(),
            },
            &context("turn-1", "call-1"),
            "sha256:compile".to_string(),
        )
        .unwrap();
    let initial = tool.working_set.workflow().unwrap();
    let run = initial.current_run.unwrap();
    let old_run_id = run.run_id.clone();
    let lineage_id = run.lineage_id.clone();
    let superseded = tool
        .execute_action(
            WorkflowStateInput::Supersede {
                expected_run_id: old_run_id.clone(),
                expected_revision: 1,
                expected_stage_id: "working".to_string(),
                reason: "Goal changed".to_string(),
                definition: definition(),
            },
            &context("turn-2", "call-2"),
            "sha256:supersede".to_string(),
        )
        .unwrap();
    assert_eq!(superseded.code, "superseded");
    let state = tool.working_set.workflow().unwrap();
    let current = state.current_run.unwrap();
    assert_eq!(current.lineage_id, lineage_id);
    assert_ne!(current.run_id, old_run_id);
    assert_eq!(current.mode.mode_id, "mode.task");
    assert_eq!(state.archived_runs.len(), 1);
}

#[test]
fn operation_identity_with_different_arguments_is_rejected() {
    let tool = tool();
    let invocation = context("turn-1", "call-1");
    let _ = tool
        .execute_action(
            WorkflowStateInput::Compile {
                expected_revision: 0,
                expected_run_id: None,
                definition: definition(),
            },
            &invocation,
            "sha256:first".to_string(),
        )
        .unwrap();
    let conflict = tool
        .execute_action(
            WorkflowStateInput::Compile {
                expected_revision: 0,
                expected_run_id: None,
                definition: definition(),
            },
            &invocation,
            "sha256:different".to_string(),
        )
        .unwrap();

    assert_eq!(conflict.code, "operationIdentityConflict");
    assert_eq!(tool.working_set.workflow().unwrap().revision, 1);
}

#[test]
fn transition_rejections_report_stable_codes_without_mutating_state() {
    let tool = tool();
    let not_compiled = tool
        .execute_action(
            transition_input("run-missing", 0, "working", "completed"),
            &context("turn-0", "call-0"),
            "sha256:not-compiled".to_string(),
        )
        .unwrap();
    assert_eq!(not_compiled.code, "workflowNotCompiled");
    assert!(tool.working_set.workflow().is_none());

    tool.execute_action(
        WorkflowStateInput::Compile {
            expected_revision: 0,
            expected_run_id: None,
            definition: indirect_definition(),
        },
        &context("turn-1", "call-1"),
        "sha256:compile".to_string(),
    )
    .unwrap();
    let run_id = tool
        .working_set
        .workflow()
        .unwrap()
        .current_run
        .unwrap()
        .run_id;

    let cases = [
        (
            transition_input("run-wrong", 1, "working", "reviewing"),
            "runMismatch",
        ),
        (
            transition_input(&run_id, 1, "planning", "reviewing"),
            "stageMismatch",
        ),
        (
            transition_input(&run_id, 1, "working", "missing"),
            "unknownTargetStage",
        ),
        (
            transition_input(&run_id, 1, "working", "completed"),
            "transitionNotAllowed",
        ),
    ];
    for (index, (input, code)) in cases.into_iter().enumerate() {
        let rejected = tool
            .execute_action(
                input,
                &context("turn-reject", &format!("call-{index}")),
                format!("sha256:reject-{index}"),
            )
            .unwrap();
        assert!(!rejected.accepted);
        assert_eq!(rejected.code, code);
        assert_eq!(rejected.operation_revision, 1);
    }

    let active_compile = tool
        .execute_action(
            WorkflowStateInput::Compile {
                expected_revision: 1,
                expected_run_id: Some(run_id),
                definition: definition(),
            },
            &context("turn-2", "call-active-compile"),
            "sha256:active-compile".to_string(),
        )
        .unwrap();
    assert_eq!(active_compile.code, "activeWorkflowExists");
    assert_eq!(tool.working_set.workflow().unwrap().revision, 1);
}

#[test]
fn terminal_run_rejects_further_transition_and_requires_compile() {
    let tool = tool();
    tool.execute_action(
        WorkflowStateInput::Compile {
            expected_revision: 0,
            expected_run_id: None,
            definition: definition(),
        },
        &context("turn-1", "call-1"),
        "sha256:compile".to_string(),
    )
    .unwrap();
    let run_id = tool
        .working_set
        .workflow()
        .unwrap()
        .current_run
        .unwrap()
        .run_id;
    tool.execute_action(
        transition_input(&run_id, 1, "working", "completed"),
        &context("turn-2", "call-2"),
        "sha256:complete".to_string(),
    )
    .unwrap();

    let rejected = tool
        .execute_action(
            transition_input(run_id, 2, "completed", "completed"),
            &context("turn-3", "call-3"),
            "sha256:after-terminal".to_string(),
        )
        .unwrap();
    assert_eq!(rejected.code, "terminalWorkflow");
    assert_eq!(tool.working_set.workflow().unwrap().revision, 2);
}

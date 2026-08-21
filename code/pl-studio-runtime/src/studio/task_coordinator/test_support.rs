use pl_core::canonical_content_hash;
use pl_protocol::AgentWorkingState;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};

use super::spawn::{
    TaskExecutorAcceptanceCriterion, TaskExecutorBlueprint, TaskExecutorImplementationStep,
    TaskExecutorScope, TaskExecutorTarget, TaskExecutorVerificationCommand,
    TaskExecutorVerificationContract,
};
use super::{TaskExecutorHandoff, TaskRun, WorkUnit};
use crate::StudioStore;

pub(crate) fn executor_blueprint(task_name: &str, scope_hint: &str) -> TaskExecutorBlueprint {
    TaskExecutorBlueprint {
        task_name: task_name.to_string(),
        objective: "move transport selection to ModelInfo".to_string(),
        scope: TaskExecutorScope {
            in_scope: vec!["model transport routing".to_string()],
            out_of_scope: Vec::new(),
            scope_hints: vec![scope_hint.to_string()],
        },
        implementation_steps: vec![TaskExecutorImplementationStep {
            id: "step-1".to_string(),
            instruction: "move transport selection to ModelInfo".to_string(),
            targets: vec![TaskExecutorTarget {
                path: scope_hint.to_string(),
                symbol: Some("ModelInfo".to_string()),
            }],
            expected_outcome: "model transport uses one canonical selector".to_string(),
            criterion_ids: vec!["criterion-1".to_string()],
        }],
        acceptance_criteria: vec![TaskExecutorAcceptanceCriterion {
            id: "criterion-1".to_string(),
            requirement: "model-level routing is tested".to_string(),
        }],
        dependencies: Vec::new(),
        evidence: Vec::new(),
        verification: TaskExecutorVerificationContract {
            commands: vec![TaskExecutorVerificationCommand {
                id: "check-1".to_string(),
                command: "cargo test -p pl-model".to_string(),
                cwd: ".".to_string(),
                purpose: "verify model transport".to_string(),
                expected_outcome: "pl-model tests pass".to_string(),
                criterion_ids: vec!["criterion-1".to_string()],
            }],
            inspections: Vec::new(),
        },
    }
}

pub(crate) async fn persist_executor_handoff(
    store: &StudioStore,
    run: &TaskRun,
    work_unit: &WorkUnit,
    parent_thread_id: &str,
) {
    let scope_hint = work_unit
        .scope_hints
        .first()
        .map(String::as_str)
        .unwrap_or(".");
    let handoff = TaskExecutorHandoff::new(
        run,
        work_unit,
        parent_thread_id.to_string(),
        executor_blueprint(&work_unit.title, scope_hint),
    )
    .unwrap()
    .to_context_section()
    .unwrap();
    let state = AgentWorkingState {
        sections: vec![handoff],
        revision: 1,
        ..AgentWorkingState::default()
    };
    let state_json = serde_json::to_string(&state).unwrap();
    crate::studio::entity::thread_session_state::ActiveModel {
        thread_id: Set(work_unit.executor_thread_id.clone().unwrap()),
        revision: Set(1),
        state_hash: Set(canonical_content_hash(state_json.as_bytes())),
        state_json: Set(state_json),
        updated_at: Set(crate::studio::ids::unix_seconds()),
    }
    .insert(store.database())
    .await
    .unwrap();
}

//! Thread 归属的 Mode 注册、状态图编译、Turn 快照与工作流工具。

mod compiler;
mod context;
mod manager;
mod registration;
mod runtime;
pub mod tools;

pub use compiler::{
    CompiledWorkflowDefinition, MAX_ARCHIVED_WORKFLOW_RUNS, MAX_WORKFLOW_DEFINITION_BYTES,
    MAX_WORKFLOW_HISTORY, MAX_WORKFLOW_OPERATION_RECEIPTS, MAX_WORKFLOW_STATE_BYTES,
    MAX_WORKFLOW_STATES, MAX_WORKFLOW_TRANSITIONS, WorkflowCompilerError, WorkflowValidationIssue,
    compile_workflow_definition,
};
pub use context::workflow_model_context_section;
pub use manager::{
    RegisteredThreadMode, ThreadModeManager, ThreadModeManagerError, ThreadModeRegistrySnapshot,
    ThreadModeSource, ThreadModeSourceId, ThreadModeSourceKind,
};
pub use registration::{
    StaticThreadModeRegistration, StaticWorkflowDefinition, StaticWorkflowState,
    StaticWorkflowTransition, ThreadModeRegistration,
};
pub use runtime::{
    archive_workflow_for_mode_change, reconcile_workflow_for_turn, validate_session_state_size,
};
pub use tools::{
    TOOL_WORKFLOW_CURRENT, TOOL_WORKFLOW_GRAPH, TOOL_WORKFLOW_HISTORY, TOOL_WORKFLOW_NEXT,
    TOOL_WORKFLOW_RESTART, TOOL_WORKFLOW_TRANSITION, WorkflowCurrentTool, WorkflowGraphTool,
    WorkflowHistoryTool, WorkflowNextTool, WorkflowRestartTool, WorkflowTransitionTool,
    validate_workflow_restart_arguments, validate_workflow_transition_arguments,
};

/// thread 行为测试共享的注册与状态图 fixture。
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;

    use pl_protocol::{
        ThreadModeId, WorkflowDefinition, WorkflowState, WorkflowStateKind, WorkflowTransition,
    };

    use super::{
        RegisteredThreadMode, ThreadModeManager, ThreadModeRegistration, ThreadModeSource,
        ThreadModeSourceId, ThreadModeSourceKind,
    };

    pub(crate) fn source(id: &str, kind: ThreadModeSourceKind) -> ThreadModeSource {
        ThreadModeSource {
            id: ThreadModeSourceId::new(id).expect("valid source id"),
            kind,
        }
    }

    /// 两状态（ready → done）最小合法状态图；`extra_guard` 用于制造图变化。
    pub(crate) fn graph(extra_guard: &str) -> WorkflowDefinition {
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

    pub(crate) fn registration(
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

    pub(crate) fn registered(
        manager: &ThreadModeManager,
        source_id: &str,
        prompt: &str,
        definition: WorkflowDefinition,
    ) -> Arc<RegisteredThreadMode> {
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
}

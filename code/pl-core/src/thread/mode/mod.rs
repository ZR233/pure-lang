//! Thread Mode 注册、状态图编译、Turn 快照与工作流工具。

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

#[cfg(test)]
mod tests;

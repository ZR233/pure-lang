mod path;
mod record;
mod supervisor;
pub mod worktree;

pub use path::AgentPath;
pub use pl_protocol::AgentStatus;
pub use record::AgentRecord;
pub(crate) use supervisor::emit_subagent_activity;
pub use supervisor::{
    AgentCloseDispositionKind, AgentCloseLifecycleRequest, AgentHandle, AgentInputBusyAction,
    AgentInputInitialAction, AgentInputQueue, AgentInputStartAttempt, AgentInputSubmission,
    AgentInputTurnMode, AgentLifecycleHook, AgentLifecycleProjection,
    AgentLifecycleProjectionRequest, AgentLifecycleStatusKind, AgentMessage, AgentMessageMode,
    AgentMessageRequest, AgentRunSpec, AgentSpawnInput, AgentSpawnLifecycleRequest,
    AgentSpawnPreparation, AgentStatusUpdate, AgentSupervisor, AgentTerminalStateChange,
    AgentToolRegistrar, AgentTurnPresence, AgentTurnStartReadiness, AgentTurnStartSnapshot,
    AgentWaitCompletion, AgentWaitLoopError, AgentWaitLoopOptions, AgentWaitLoopResult,
    AgentWaitOutcome, AgentWaitSnapshot, wait_for_agent_completion,
};
pub use worktree::{
    CloseDisposition, CloseOutcome, LocalWorktreeBackend, MergeOutcome, WorktreeBackend,
    WorktreeCreateFailure, WorktreeCreateSpec, WorktreeError, WorktreeHandle, WorktreeManager,
    WorktreeRef,
};

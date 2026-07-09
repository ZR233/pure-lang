mod path;
mod record;
mod supervisor;
pub mod worktree;

pub use path::AgentPath;
pub use pl_protocol::AgentStatus;
pub use record::AgentRecord;
pub(crate) use supervisor::emit_subagent_activity;
pub use supervisor::{
    AgentHandle, AgentInputBusyAction, AgentInputInitialAction, AgentInputQueue,
    AgentInputStartAttempt, AgentInputSubmission, AgentInputTurnMode, AgentLifecycleStatusKind,
    AgentMessage, AgentMessageMode, AgentMessageRequest, AgentRunSpec, AgentSpawnInput,
    AgentStatusUpdate, AgentSupervisor, AgentToolRegistrar, AgentTurnPresence,
    AgentTurnStartReadiness, AgentTurnStartSnapshot, AgentWaitCompletion, AgentWaitLoopError,
    AgentWaitLoopOptions, AgentWaitLoopResult, AgentWaitOutcome, AgentWaitSnapshot,
    wait_for_agent_completion,
};
pub use worktree::{
    CloseDisposition, CloseOutcome, LocalWorktreeBackend, MergeOutcome, WorktreeBackend,
    WorktreeError, WorktreeHandle, WorktreeManager, WorktreeRef,
};

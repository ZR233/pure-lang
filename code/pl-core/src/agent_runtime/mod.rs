//! 产品无关的 agent actor runtime。

mod agent_loop;
mod collaboration;
mod coordinator;
mod directory;
mod handle;
mod host;
mod id;
mod policy;
mod runtime;
mod state;
mod turn;

#[cfg(test)]
mod tests;

pub use collaboration::AgentCollaborationTools;
pub use directory::{AgentDirectorySnapshot, AgentDirectorySubscription};
pub use handle::AgentRuntimeHandle;
pub use host::{
    AgentCommit, AgentCommitObserver, AgentCommitOutcome, AgentCommittedEvent,
    AgentLifecycleAdapter, AgentRuntimeHost, AgentStateMutation, AgentStateRepository,
    AgentTurnFactory, CloseLifecycleRequest, RestoredAgentRuntime, RestoredSessionProjection,
    SessionProjectionCommit, SpawnLifecycleRequest,
};
pub use id::{AgentId, SessionId, TurnId};
pub use policy::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentTargetSelector, ToolEffectSet,
    TurnFinalizationPolicy,
};
pub use runtime::{AgentRuntime, AgentRuntimeOptions, RestoredInputPolicy};
pub use state::{
    AgentActivityState, AgentCurrentSessionSubmitRequest, AgentDirectoryWaitReason,
    AgentDirectoryWaitResult, AgentDurableState, AgentIdentity, AgentLifecycleState,
    AgentProgressCheckpoint, AgentProgressStage, AgentRegistration, AgentRuntimeError,
    AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeResult, AgentSessionDigest,
    AgentSessionDigestMessage, AgentSessionDigestRole, AgentSessionState, AgentSnapshot,
    AgentSpawnRequest, AgentSpawnResult, AgentSubmitRequest, AgentTurnOutcome, AgentWaitResult,
    DurableMailboxEnvelope, MailboxDeliveryState, MailboxPresentation, PendingAgentInput,
    TurnOutcomeKind,
};
pub(crate) use turn::AgentTurnMailboxHandle;
pub use turn::{
    AgentSessionCommitPolicy, AgentTurnCheckpoint, AgentTurnCheckpointHandle,
    AgentTurnPreparationContext, PreparedAgentTurn, PreparedSessionRuntime, TurnCheckpointReason,
};

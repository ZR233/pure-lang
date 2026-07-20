//! 产品无关的 agent actor runtime。

mod actor;
mod collaboration;
mod coordinator;
mod execution;
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
pub use handle::AgentRuntimeHandle;
pub use host::{
    AgentCommit, AgentCommitOutcome, AgentCommittedEvent, AgentEventSink, AgentLifecycleAdapter,
    AgentRuntimeHost, AgentStateMutation, AgentStateRepository, AgentTurnFactory,
    CloseLifecycleRequest, RestoredAgentRuntime, SpawnLifecycleRequest,
};
pub use id::{AgentId, SessionId, TurnId};
pub use policy::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentTargetSelector, ToolEffectSet,
    TurnFinalizationPolicy,
};
pub use runtime::{AgentRuntime, AgentRuntimeOptions, RestoredInputPolicy};
pub use state::{
    AgentActivityState, AgentDurableState, AgentIdentity, AgentLifecycleState, AgentRegistration,
    AgentRuntimeError, AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeResult,
    AgentSessionState, AgentSnapshot, AgentSpawnRequest, AgentSpawnResult, AgentSubmitRequest,
    AgentTurnOutcome, AgentWaitResult, InputDelivery, PendingAgentInput, TurnOutcomeKind,
};
pub use turn::{
    AgentSessionCommitPolicy, AgentTurnCheckpoint, AgentTurnCheckpointHandle,
    AgentTurnPreparationContext, PreparedAgentTurn, TurnCheckpointReason,
};

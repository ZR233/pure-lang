//! 产品无关的 agent actor runtime。

mod actor;
mod collaboration;
mod coordinator;
mod event_hub;
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
pub use event_hub::{
    AgentParentSubscription, AgentSubscriptionItem, AgentUpdateEnvelope, AgentUpdateKind,
    AgentWakeBatch, AgentWakeReason,
};
pub use handle::AgentRuntimeHandle;
pub use host::{
    AgentCommit, AgentCommitObserver, AgentCommitOutcome, AgentCommittedEvent,
    AgentLifecycleAdapter, AgentRuntimeHost, AgentStateMutation, AgentStateRepository,
    AgentTurnFactory, CloseLifecycleRequest, RestoredAgentRuntime, RestoredSessionProjection,
    SessionProjectionCommit, SpawnLifecycleRequest,
};
pub use id::{AgentId, AgentWakeId, SessionId, TurnId};
pub use policy::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentTargetSelector, ToolEffectSet,
    TurnFinalizationPolicy,
};
pub use runtime::{AgentRuntime, AgentRuntimeOptions, RestoredInputPolicy};
pub use state::{
    AcceptedAgentWake, AgentActivityState, AgentCurrentSessionSubmitRequest, AgentDurableState,
    AgentIdentity, AgentLifecycleState, AgentRegistration, AgentRuntimeError, AgentRuntimeEvent,
    AgentRuntimeEventKind, AgentRuntimeResult, AgentSessionState, AgentSnapshot, AgentSpawnRequest,
    AgentSpawnResult, AgentSubmitRequest, AgentTurnOutcome, AgentWaitResult, AgentWakePolicy,
    InputDelivery, PendingAgentInput, TurnOutcomeKind,
};
pub use turn::{
    AgentSessionCommitPolicy, AgentTurnCheckpoint, AgentTurnCheckpointHandle,
    AgentTurnPreparationContext, PreparedAgentTurn, PreparedSessionRuntime, TurnCheckpointReason,
};

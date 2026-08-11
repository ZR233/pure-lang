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
    AgentCommitObserver, AgentCommittedEvent, AgentLifecycleAdapter, AgentRuntimeHost,
    AgentTurnFactory, CloseLifecycleRequest, DurableCommitFacts, RestoredAgentRuntime,
    RestoredThreadSnapshot, SpawnLifecycleRequest, ThreadCommit, ThreadCommitOutcome,
    ThreadContextMutation, ThreadMutation, ThreadProjectionCommit, ThreadRepository,
};
pub use id::{AgentId, ThreadId, TurnId};
pub use policy::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentTargetSelector, ToolEffectSet,
    TurnFinalizationPolicy,
};
pub use runtime::{AgentRuntime, AgentRuntimeOptions, RestoredInputPolicy};
pub use state::*;
pub(crate) use turn::AgentTurnMailboxHandle;
pub use turn::{
    AgentInferenceCommit, AgentSessionCommitPolicy, AgentTurnCheckpoint, AgentTurnCheckpointHandle,
    AgentTurnPreparationContext, PreparedAgentTurn, PreparedSessionRuntime, TurnCheckpointReason,
};

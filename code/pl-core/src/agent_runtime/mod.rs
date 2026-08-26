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

pub use collaboration::{AgentCollaborationToolConfig, AgentCollaborationTools};
pub use directory::{AgentDirectorySnapshot, AgentDirectorySubscription};
pub use handle::AgentRuntimeHandle;
pub use host::{
    AgentCommitObserver, AgentCommittedEvent, AgentLifecycleAdapter, AgentRuntimeHost,
    AgentTurnFactory, CloseLifecycleRequest, DurableCommitFacts, PersistenceClass,
    RestoredAgentRuntime, RestoredThreadSnapshot, SpawnLifecycleRequest, SpawnRollbackPhase,
    SpawnRollbackReason, ThreadCommit, ThreadContextMutation, ThreadMutation,
    ThreadProjectionCommit, ThreadRepository,
};
pub use id::{ThreadId, TurnId};
pub use policy::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentTargetSelector, ToolEffectSet,
    TurnFinalizationPolicy,
};
pub use runtime::{AgentRuntime, AgentRuntimeOptions, RestoredInputPolicy};
pub use state::*;
pub use turn::{
    AgentInferenceCommit, AgentSessionCommitPolicy, AgentTurnCheckpoint, AgentTurnCheckpointHandle,
    AgentTurnPreparationContext, PreparedAgentTurn, PreparedSessionRuntime, TurnCheckpointReason,
};
pub(crate) use turn::{
    AgentTurnMailboxHandle, TurnBudgetRefreshHandle, TurnBudgetRefreshReceiver,
    turn_budget_refresh_channel,
};

#[cfg(test)]
mod unit_tests;

//! 待保存事实只保留上下文增量和工作状态，不保留每个版本的完整对话副本。

use std::collections::VecDeque;

use pl_core::{
    AgentSession, AgentSnapshot, DurableCommitFacts, DurableMailboxEnvelope, PersistenceClass,
    ThreadActorState, ThreadCommit, ThreadContextMetadata, ThreadContextMutation,
    ThreadContextState, ThreadId, ThreadMutation,
};
use pl_protocol::{AgentSessionSnapshot, AgentWorkingState, InferenceTokenUsage};

use crate::PureError;

#[derive(Debug, Clone)]
pub(super) struct ThreadFact {
    pub(super) agent_id: ThreadId,
    pub(super) persistence: PersistenceClass,
    expected_revision: Option<u64>,
    pub(super) facts: DurableCommitFacts,
    mutation: ThreadMutation,
    snapshot: AgentSnapshot,
    metadata: ThreadContextMetadata,
    working_state: AgentWorkingState,
    usage: InferenceTokenUsage,
    last_context_tokens: Option<u64>,
    trace_sequence: u64,
    thread_revision: u64,
    pending_inputs: VecDeque<DurableMailboxEnvelope>,
    active_input: Option<DurableMailboxEnvelope>,
}

impl From<ThreadCommit> for ThreadFact {
    fn from(commit: ThreadCommit) -> Self {
        let ThreadActorState {
            snapshot,
            session,
            pending_inputs,
            active_input,
        } = commit.next_state;
        let mut facts = commit.facts;
        // 通知包含所有展示变更，完整投影只是提交时的校验产物。
        facts.projection_snapshot = None;
        Self {
            agent_id: commit.agent_id,
            persistence: commit.persistence,
            expected_revision: commit.expected_revision,
            facts,
            mutation: commit.mutation,
            snapshot,
            metadata: session.metadata,
            working_state: session.session.working_state().clone(),
            usage: session.usage,
            last_context_tokens: session.last_context_tokens,
            trace_sequence: session.trace_sequence,
            thread_revision: session.thread_revision,
            pending_inputs,
            active_input,
        }
    }
}

impl ThreadFact {
    /// 只在后台事务中临时物化完整上下文；活动内存永不读取这个结果。
    pub(super) async fn materialize(
        &self,
        tx: &sea_orm::DatabaseTransaction,
    ) -> Result<ThreadCommit, PureError> {
        use sea_orm::EntityTrait;
        let row = crate::studio::entity::thread::Entity::find_by_id(self.agent_id.to_string())
            .one(tx)
            .await
            .map_err(super::super::store_error)?;
        let already_applied = row
            .and_then(|row| row.runtime_revision)
            .is_some_and(|revision| u64::try_from(revision).ok() == Some(self.facts.revision));
        let transcript = if already_applied {
            super::super::context::restore_transcript(tx, self.agent_id.as_str()).await?
        } else {
            match &self.facts.context {
                Some(ThreadContextMutation::Replace { items }) => items.clone(),
                mutation @ (Some(ThreadContextMutation::Append { .. }) | None) => {
                    let mut items =
                        super::super::context::restore_transcript(tx, self.agent_id.as_str())
                            .await?;
                    if let Some(ThreadContextMutation::Append { items: suffix }) = mutation {
                        items.extend_from_slice(suffix);
                    }
                    items
                }
            }
        };
        Ok(ThreadCommit {
            agent_id: self.agent_id.clone(),
            persistence: self.persistence,
            expected_revision: self.expected_revision,
            facts: self.facts.clone(),
            mutation: self.mutation.clone(),
            next_state: ThreadActorState {
                snapshot: self.snapshot.clone(),
                pending_inputs: self.pending_inputs.clone(),
                active_input: self.active_input.clone(),
                session: ThreadContextState {
                    metadata: self.metadata.clone(),
                    submissions: Default::default(),
                    billing_by_turn: Default::default(),
                    session: AgentSession::from_snapshot(AgentSessionSnapshot {
                        transcript,
                        working_state: self.working_state.clone(),
                    }),
                    usage: self.usage.clone(),
                    last_context_tokens: self.last_context_tokens,
                    trace_sequence: self.trace_sequence,
                    thread_revision: self.thread_revision,
                },
            },
        })
    }
}

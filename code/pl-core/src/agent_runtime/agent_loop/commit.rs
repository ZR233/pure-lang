use pl_protocol::ThreadNotificationEnvelope;
use pl_trace::TraceEvent;

use super::super::host::{AgentCommitObserver, PersistenceClass, ThreadRepository};
use super::super::state::AgentRuntimeError;
use super::super::{
    AgentCommittedEvent, AgentRuntimeEvent, AgentRuntimeHost, AgentRuntimeResult,
    DurableCommitFacts, ThreadActorState, ThreadCommit, ThreadId, ThreadMutation, TurnId,
};
use super::AgentLoop;

/// 一次内存 commit 成功后需要广播的 typed 事实。
///
/// Builder 只描述发布内容和 Directory 更新策略；真正的发布顺序由
/// [`AgentLoop::commit_and_publish`] 统一控制。
pub(super) struct CommitPublication {
    directory_update: DirectoryUpdate,
    thread_id: Option<ThreadId>,
    turn_id: Option<TurnId>,
    runtime_events: Vec<AgentRuntimeEvent>,
    trace_events: Vec<TraceEvent>,
    thread_notifications: Vec<ThreadNotificationEnvelope>,
}

enum DirectoryUpdate {
    Unchanged,
    StoreSnapshot,
    RuntimeEvent(Box<AgentRuntimeEvent>),
}

impl CommitPublication {
    pub(super) fn new(thread_id: Option<ThreadId>, turn_id: Option<TurnId>) -> Self {
        Self {
            directory_update: DirectoryUpdate::Unchanged,
            thread_id,
            turn_id,
            runtime_events: Vec::new(),
            trace_events: Vec::new(),
            thread_notifications: Vec::new(),
        }
    }

    pub(super) fn store_directory_snapshot(mut self) -> Self {
        self.directory_update = DirectoryUpdate::StoreSnapshot;
        self
    }

    pub(super) fn with_runtime_event(mut self, event: AgentRuntimeEvent) -> Self {
        self.directory_update = DirectoryUpdate::RuntimeEvent(Box::new(event.clone()));
        self.runtime_events.push(event);
        self
    }

    pub(super) fn with_trace_events(mut self, events: Vec<TraceEvent>) -> Self {
        self.trace_events = events;
        self
    }

    pub(super) fn with_thread_notifications(
        mut self,
        notifications: Vec<ThreadNotificationEnvelope>,
    ) -> Self {
        self.thread_notifications = notifications;
        self
    }
}

/// 已准备完成、待加入内存待落库队列的提交命令。
pub(super) struct PendingCommit {
    persistence: PersistenceClass,
    next_state: ThreadActorState,
    facts: DurableCommitFacts,
    mutation: ThreadMutation,
    publication: Option<CommitPublication>,
}

impl PendingCommit {
    pub(super) fn new(
        next_state: ThreadActorState,
        facts: DurableCommitFacts,
        mutation: ThreadMutation,
    ) -> Self {
        Self {
            persistence: PersistenceClass::Coalescible,
            next_state,
            facts,
            mutation,
            publication: None,
        }
    }

    /// 覆盖默认持久化分类。
    pub(super) fn persistence(mut self, persistence: PersistenceClass) -> Self {
        self.persistence = persistence;
        self
    }

    pub(super) fn publish(mut self, publication: CommitPublication) -> Self {
        self.publication = Some(publication);
        self
    }
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    /// 统一执行内存待落库入队与状态/事件发布模板。
    ///
    /// 调用方负责准备领域 state、facts 与 mutation；repository 只接受到进程内
    /// 待落库队列，不等待 SQLite。接受后替换 owner snapshot 并立即广播同一事实。
    pub(super) async fn commit_and_publish(
        &mut self,
        commit: PendingCommit,
    ) -> AgentRuntimeResult<()> {
        let expected_revision = self.state.snapshot.revision;
        debug_assert_eq!(
            commit.next_state.snapshot.revision,
            expected_revision.saturating_add(1),
            "pending commit must advance the actor revision exactly once"
        );
        self.host
            .repository()
            .commit(ThreadCommit {
                agent_id: commit.next_state.snapshot.identity.id.clone(),
                persistence: commit.persistence,
                expected_revision: Some(expected_revision),
                next_state: commit.next_state.clone(),
                facts: commit.facts,
                mutation: commit.mutation,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;

        self.state = commit.next_state;
        let Some(publication) = commit.publication else {
            return Ok(());
        };
        match &publication.directory_update {
            DirectoryUpdate::Unchanged => {}
            DirectoryUpdate::StoreSnapshot => self
                .runtime
                .directory
                .store_snapshot(self.state.snapshot.clone()),
            DirectoryUpdate::RuntimeEvent(event) => {
                self.runtime.directory.publish_runtime_event(event);
            }
        }
        if let Err(error) = self
            .runtime
            .thread_events
            .publish_batch(publication.thread_notifications.clone())
            .await
        {
            tracing::error!(
                agent_id = %self.state.snapshot.identity.id,
                revision = self.state.snapshot.revision,
                error = %error,
                "thread projection rejected a committed in-memory fact; subscribers must resync"
            );
        }
        self.host
            .observer()
            .publish(AgentCommittedEvent {
                agent_id: self.state.snapshot.identity.id.clone(),
                thread_id: publication.thread_id,
                turn_id: publication.turn_id,
                runtime_events: publication.runtime_events,
                trace_events: publication.trace_events,
                thread_notifications: publication.thread_notifications,
            })
            .await;
        Ok(())
    }
}

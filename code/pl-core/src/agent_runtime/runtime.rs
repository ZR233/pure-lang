use std::collections::VecDeque;
use std::time::Duration;

use super::coordinator::spawn_coordinator;
use super::host::{AgentCommitObserver, PersistenceClass, ThreadRepository};
use super::state::{AgentRuntimeError, unix_timestamp};
use super::{
    AgentCommand, AgentCommittedEvent, AgentRuntimeEvent, AgentRuntimeEventKind,
    AgentRuntimeHandle, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshotTransition, AgentState,
    AgentTurnOutcome, MailboxDeliveryState, RestoredAgentRuntime, ThreadId, TurnId,
};
use crate::thread_event::{project_runtime_event, runtime_event_thread_id};
use crate::{ThreadEventBus, ThreadEventBusHandle, ThreadEventOptions};

/// runtime 启动时如何处理 repository 恢复出的 pending inputs。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RestoredInputPolicy {
    /// host 已完成外部资源恢复，可立即按 FIFO 执行。
    #[default]
    Start,
    /// 暂停恢复队列，等待产品调用 `start_restored_inputs` 放行。
    Hold,
}

/// agent runtime 的并发和取消参数。
#[derive(Debug, Clone, Copy)]
pub struct AgentRuntimeOptions {
    pub command_capacity: usize,
    pub cancel_grace: Duration,
    pub restored_inputs: RestoredInputPolicy,
    pub thread_events: ThreadEventOptions,
}

impl Default for AgentRuntimeOptions {
    fn default() -> Self {
        Self {
            command_capacity: 128,
            cancel_grace: Duration::from_millis(500),
            restored_inputs: RestoredInputPolicy::Start,
            thread_events: ThreadEventOptions::default(),
        }
    }
}

/// 持有泛型 host 的 agent runtime 所有者。
///
/// 产品通常保留该值，并把 cloneable `AgentRuntimeHandle` 交给 facade 与工具。
#[derive(Debug)]
pub struct AgentRuntime<H>
where
    H: AgentRuntimeHost,
{
    host: H,
    handle: AgentRuntimeHandle,
}

impl<H> AgentRuntime<H>
where
    H: AgentRuntimeHost,
{
    /// 从 repository 恢复 durable agents，收束遗留 Running turn，并启动 actors。
    pub async fn start(host: H, options: AgentRuntimeOptions) -> AgentRuntimeResult<Self> {
        let mut restored = host
            .repository()
            .restore_runtime()
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        let thread_events = ThreadEventBus::new(options.thread_events);
        let thread_event_handle = thread_events.handle();
        for agent in &mut restored {
            let snapshot = normalize_restored_thread_snapshot(agent)?;
            thread_event_handle
                .replace_snapshot(snapshot.clone())
                .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        }
        let restored = recover_interrupted_turns(&host, &thread_event_handle, restored).await?;
        let handle = spawn_coordinator(host.clone(), restored, options, thread_events)?;
        Ok(Self { host, handle })
    }

    /// 返回不暴露 host 泛型的 runtime 命令句柄。
    pub fn handle(&self) -> AgentRuntimeHandle {
        self.handle.clone()
    }

    /// 返回 host，供产品 facade 读取自身只读能力。
    pub fn host(&self) -> &H {
        &self.host
    }

    /// 停止 runtime。
    pub async fn shutdown(&self) -> AgentRuntimeResult<()> {
        self.handle.shutdown().await
    }
}

pub(super) fn normalize_restored_thread_snapshot(
    agent: &mut RestoredAgentRuntime,
) -> AgentRuntimeResult<pl_protocol::ThreadSnapshot> {
    let snapshot = agent
        .thread_snapshot
        .take()
        .map(|restored| restored.snapshot)
        .unwrap_or_else(|| {
            let mut snapshot =
                pl_protocol::ThreadSnapshot::empty(agent.state.snapshot.identity.id.as_str());
            snapshot.revision = agent.state.session.thread_revision;
            snapshot
        });
    let thread_id = ThreadId::new(snapshot.thread.id.clone())
        .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
    if agent.state.snapshot.identity.id != thread_id {
        return Err(AgentRuntimeError::ThreadMismatch {
            agent_id: agent.state.snapshot.identity.id.clone(),
            expected: agent.state.snapshot.identity.id.clone(),
            actual: thread_id,
        });
    }
    if agent.state.session.thread_revision != snapshot.revision {
        tracing::warn!(
            agent_id = %agent.state.snapshot.identity.id,
            thread_id = %agent.state.snapshot.identity.id,
            checkpoint = agent.state.session.thread_revision,
            canonical = snapshot.revision,
            "repairing stale thread revision during restore"
        );
        agent.state.session.thread_revision = snapshot.revision;
    }
    agent.thread_snapshot = Some(super::RestoredThreadSnapshot {
        snapshot: snapshot.clone(),
    });
    Ok(snapshot)
}

/// 收束恢复出的遗留 active Turn/Claimed input，并派生 `RecoveryCancelledTurn`。
///
/// 启动恢复与惰性驻留的按需恢复共用本函数。
pub(crate) async fn recover_interrupted_turns<H>(
    host: &H,
    thread_events: &ThreadEventBusHandle,
    restored: Vec<RestoredAgentRuntime>,
) -> AgentRuntimeResult<Vec<RestoredAgentRuntime>>
where
    H: AgentRuntimeHost,
{
    let mut recovered = Vec::with_capacity(restored.len());
    for mut agent in restored {
        let interrupted_turn_id = agent.state.snapshot.active_turn_id().cloned();
        let mut pending_inputs = VecDeque::new();
        while let Some(mut input) = agent.state.pending_inputs.pop_front() {
            if input.mail_id.trim().is_empty() {
                input.mail_id = format!("mail:{}", input.turn_id);
            }
            match input.delivery_state {
                MailboxDeliveryState::Pending(_) => pending_inputs.push_back(input),
                MailboxDeliveryState::Claimed(_) => {
                    let requeued_turn_id = if interrupted_turn_id.as_ref() == Some(&input.turn_id) {
                        TurnId::generate()
                    } else {
                        input.turn_id.clone()
                    };
                    input
                        .requeue(requeued_turn_id)
                        .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
                    pending_inputs.push_back(input);
                }
                MailboxDeliveryState::Consumed(_) => {}
            }
        }
        agent.state.pending_inputs = pending_inputs;
        agent.state.refresh_mailbox_snapshot();
        let had_active_input = agent.state.active_input.is_some();
        let interrupted = had_active_input
            || matches!(
                agent.state.snapshot.state,
                AgentState::Running(_)
                    | AgentState::WaitingTool(_)
                    | AgentState::WaitingInteraction(_)
                    | AgentState::Cancelling(_)
            );
        if !interrupted {
            recovered.push(agent);
            continue;
        }
        agent.state.active_input = None;
        let turn_id = agent
            .state
            .snapshot
            .active_turn_id()
            .cloned()
            .unwrap_or_else(TurnId::generate);
        let thread_id = agent.state.snapshot.identity.id.clone();
        let outcome = AgentTurnOutcome {
            turn_id,
            thread_id,
            outcome: pl_protocol::TurnOutcome::cancelled(
                pl_protocol::TurnCancellationCause::Recovery,
            ),
            usage: pl_model::TokenUsage::default(),
            started_at: None,
            finished_at: unix_timestamp(),
        };
        let expected_revision = agent.state.snapshot.revision;
        agent.state.snapshot.revision = expected_revision.saturating_add(1);
        agent.state.snapshot.event_sequence = agent.state.snapshot.event_sequence.saturating_add(1);
        agent.state.snapshot.last_turn = Some(outcome.clone());
        agent.state.refresh_mailbox_snapshot();
        let next_turn_id = agent.state.triggering_turn_id();
        agent
            .state
            .snapshot
            .transition(AgentCommand::Settle { next_turn_id })
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        agent.state.snapshot.updated_at = unix_timestamp();
        let event = AgentRuntimeEvent {
            agent_id: agent.state.snapshot.identity.id.clone(),
            sequence: agent.state.snapshot.event_sequence,
            created_at: agent.state.snapshot.updated_at,
            kind: AgentRuntimeEventKind::RecoveryCancelledTurn {
                outcome,
                snapshot: Box::new(agent.state.snapshot.clone()),
            },
        };
        let thread_id = runtime_event_thread_id(&event)
            .ok_or_else(|| {
                AgentRuntimeError::ThreadEvents(
                    "recovery cancellation is missing its canonical thread".to_string(),
                )
            })?
            .to_string();
        let thread_key = ThreadId::new(thread_id.clone())
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        let current_thread = thread_events
            .snapshot(&thread_id)
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let projected = project_runtime_event(&event, &current_thread);
        let projected_thread = thread_events
            .project(&thread_id, &projected.notifications)
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let thread_notifications = projected_thread.notifications.clone();
        let projection = super::ThreadProjectionCommit {
            snapshot: projected_thread.snapshot,
            notifications: projected_thread.notifications,
        };
        if agent.state.snapshot.identity.id != thread_key {
            return Err(AgentRuntimeError::ThreadMismatch {
                agent_id: agent.state.snapshot.identity.id.clone(),
                expected: agent.state.snapshot.identity.id.clone(),
                actual: thread_key,
            });
        }
        agent.state.session.thread_revision = projected.through_revision;
        host.repository()
            .commit(super::ThreadCommit {
                agent_id: agent.state.snapshot.identity.id.clone(),
                persistence: PersistenceClass::Settlement,
                expected_revision: Some(expected_revision),
                next_state: agent.state.clone(),
                facts: super::DurableCommitFacts::from_state(
                    &agent.state,
                    vec![event.clone()],
                    Vec::new(),
                    Some(projection.clone()),
                    None,
                ),
                mutation: super::ThreadMutation::SnapshotAndQueue,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        // 恢复事件与普通 actor commit 使用同一个 parent subscription 事实源。
        if let Err(error) = thread_events
            .publish_batch(projected.notifications.clone())
            .await
        {
            tracing::error!(
                agent_id = %agent.state.snapshot.identity.id,
                revision = agent.state.snapshot.revision,
                error = %error,
                "recovery projection rejected a committed in-memory fact; subscribers must resync"
            );
        }
        host.observer()
            .publish(AgentCommittedEvent {
                agent_id: event.agent_id.clone(),
                thread_id: Some(thread_key),
                turn_id: projected
                    .notifications
                    .first()
                    .and_then(notification_turn_id)
                    .and_then(|value| TurnId::new(value).ok()),
                runtime_events: vec![event],
                trace_events: Vec::new(),
                thread_notifications: thread_notifications.clone(),
            })
            .await;
        if let Some(restored_thread) = agent.thread_snapshot.as_mut() {
            restored_thread.snapshot = projection.snapshot;
        } else {
            agent.thread_snapshot = Some(super::RestoredThreadSnapshot {
                snapshot: projection.snapshot,
            });
        }
        recovered.push(agent);
    }
    Ok(recovered)
}

fn notification_turn_id(notification: &pl_protocol::ThreadNotificationEnvelope) -> Option<String> {
    match &notification.notification {
        pl_protocol::ThreadNotification::TurnStarted { turn }
        | pl_protocol::ThreadNotification::TurnUpdated { turn }
        | pl_protocol::ThreadNotification::TurnCompleted { turn } => Some(turn.id.clone()),
        pl_protocol::ThreadNotification::ItemStarted { item }
        | pl_protocol::ThreadNotification::ItemCompleted { item } => Some(item.turn_id.clone()),
        pl_protocol::ThreadNotification::ItemDelta { .. }
        | pl_protocol::ThreadNotification::InteractionChanged { .. }
        | pl_protocol::ThreadNotification::ThreadRuntimeUpdated { .. }
        | pl_protocol::ThreadNotification::Lagged { .. } => None,
    }
}

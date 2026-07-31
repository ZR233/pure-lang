use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use super::{
    AgentActivityState, AgentId, AgentRuntimeError, AgentRuntimeEvent, AgentRuntimeEventKind,
    AgentRuntimeResult, AgentSnapshot, AgentTurnOutcome, AgentWakeId, AgentWakePolicy,
};

const EVENT_CAPACITY: usize = 256;
const MAX_SUMMARY_CHARS: usize = 2_048;

/// 会触发父代理重新评估工作的子代理更新类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum AgentUpdateKind {
    ActivityChanged { activity: AgentActivityState },
    ProgressReported,
    TodoPhaseChanged,
    NeedsAttention,
    RuntimeTerminal { outcome: Option<AgentTurnOutcome> },
    ProductPhaseChanged { phase: String },
}

/// durable commit 之后发布给父代理订阅者的规范更新。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUpdateEnvelope {
    pub signal_id: String,
    pub parent_agent_id: AgentId,
    pub agent_id: AgentId,
    pub agent_revision: u64,
    pub event_sequence: u64,
    pub occurred_at: i64,
    pub kind: AgentUpdateKind,
    pub snapshot: AgentSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// 唤醒 Planner 时注入的合并原因。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum AgentWakeReason {
    Updates,
    InactivityDiagnostic { timed_out_agent_ids: Vec<AgentId> },
}

/// Planner continuation 使用的有界、typed 子代理上下文。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentWakeContext {
    pub current_agent_states: Vec<AgentSnapshot>,
    pub wake_reason: AgentWakeReason,
    pub last_activity_at: BTreeMap<AgentId, i64>,
    pub recent_progress: Vec<AgentUpdateEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_commentary: Option<String>,
    pub terminal_facts: Vec<AgentUpdateEnvelope>,
    pub user_stop_requested: bool,
    pub signal_revision: u64,
    pub lag_reconciled: bool,
    pub diagnostic_only: bool,
}

/// 一次 Planner 续轮消费的全部子代理更新。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentWakeBatch {
    pub wake_id: AgentWakeId,
    pub parent_agent_id: AgentId,
    pub reason: AgentWakeReason,
    pub updates: Vec<AgentUpdateEnvelope>,
    pub children: Vec<AgentSnapshot>,
    pub context: AgentWakeContext,
}

/// 父代理订阅的首帧与实时接收端。
pub struct AgentParentSubscription {
    pub children: Vec<AgentSnapshot>,
    pub through_sequence: u64,
    receiver: broadcast::Receiver<AgentUpdateEnvelope>,
}

/// direct-child 订阅的实时帧；stale 要求调用方重读 canonical snapshots。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSubscriptionItem {
    Update(Box<AgentUpdateEnvelope>),
    Stale,
}

impl AgentParentSubscription {
    pub async fn recv(&mut self) -> AgentRuntimeResult<AgentSubscriptionItem> {
        match self.receiver.recv().await {
            Ok(update) => Ok(AgentSubscriptionItem::Update(Box::new(update))),
            Err(broadcast::error::RecvError::Lagged(_)) => Ok(AgentSubscriptionItem::Stale),
            Err(broadcast::error::RecvError::Closed) => Err(AgentRuntimeError::ChannelClosed),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentEventHubHandle {
    inner: Arc<AgentEventHubInner>,
}

#[derive(Debug)]
struct AgentEventHubInner {
    snapshots: RwLock<BTreeMap<AgentId, AgentSnapshot>>,
    quiesced_parent_waits: RwLock<BTreeSet<AgentId>>,
    sender: broadcast::Sender<AgentUpdateEnvelope>,
    parent_senders: RwLock<BTreeMap<AgentId, broadcast::Sender<AgentUpdateEnvelope>>>,
    parent_wait_sender: broadcast::Sender<AgentId>,
    runtime_sender: broadcast::Sender<AgentRuntimeEvent>,
    snapshot_sender: broadcast::Sender<AgentSnapshot>,
}

impl AgentEventHubHandle {
    pub(crate) fn new(restored: impl IntoIterator<Item = AgentSnapshot>) -> Self {
        let (sender, _) = broadcast::channel(EVENT_CAPACITY);
        let (parent_wait_sender, _) = broadcast::channel(EVENT_CAPACITY);
        let (runtime_sender, _) = broadcast::channel(EVENT_CAPACITY);
        let (snapshot_sender, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            inner: Arc::new(AgentEventHubInner {
                snapshots: RwLock::new(
                    restored
                        .into_iter()
                        .map(|snapshot| (snapshot.identity.id.clone(), snapshot))
                        .collect(),
                ),
                quiesced_parent_waits: RwLock::new(BTreeSet::new()),
                sender,
                parent_senders: RwLock::new(BTreeMap::new()),
                parent_wait_sender,
                runtime_sender,
                snapshot_sender,
            }),
        }
    }

    pub(crate) fn publish_runtime_event(&self, event: &AgentRuntimeEvent) {
        let snapshot = snapshot_for_event(&event.kind);
        let previous = self
            .inner
            .snapshots
            .read()
            .expect("agent event snapshots lock poisoned")
            .get(&snapshot.identity.id)
            .cloned();
        if previous.as_ref().is_some_and(|previous| {
            previous.revision > snapshot.revision
                || (previous.revision == snapshot.revision
                    && previous.event_sequence >= snapshot.event_sequence)
        }) {
            return;
        }
        self.store_snapshot(snapshot.clone());
        let _ = self.inner.runtime_sender.send(event.clone());
        let Some(parent_agent_id) = snapshot.identity.parent_id.clone() else {
            return;
        };
        let kind = match &event.kind {
            AgentRuntimeEventKind::StateChanged { snapshot } => {
                if previous
                    .as_ref()
                    .is_some_and(|previous| previous.activity == snapshot.activity)
                {
                    None
                } else {
                    Some(
                        if snapshot.activity == AgentActivityState::WaitingInteraction {
                            AgentUpdateKind::NeedsAttention
                        } else {
                            AgentUpdateKind::ActivityChanged {
                                activity: snapshot.activity,
                            }
                        },
                    )
                }
            }
            AgentRuntimeEventKind::TurnFinished { outcome, .. }
            | AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. }
                if snapshot.wake_policy == AgentWakePolicy::RuntimeTerminal =>
            {
                Some(AgentUpdateKind::RuntimeTerminal {
                    outcome: Some(outcome.clone()),
                })
            }
            AgentRuntimeEventKind::Faulted { .. }
                if snapshot.wake_policy == AgentWakePolicy::RuntimeTerminal =>
            {
                Some(AgentUpdateKind::RuntimeTerminal {
                    outcome: snapshot.last_turn.clone(),
                })
            }
            AgentRuntimeEventKind::TurnFinished { .. }
            | AgentRuntimeEventKind::RecoveryCancelledTurn { .. }
            | AgentRuntimeEventKind::Faulted { .. } => None,
            AgentRuntimeEventKind::Registered { .. }
            | AgentRuntimeEventKind::TurnQueued { .. }
            | AgentRuntimeEventKind::TurnStarted { .. }
            | AgentRuntimeEventKind::SessionOpened { .. } => None,
        };
        let Some(kind) = kind else {
            return;
        };
        self.send(AgentUpdateEnvelope {
            signal_id: format!("runtime:{}:{}", event.agent_id, event.sequence),
            parent_agent_id,
            agent_id: event.agent_id.clone(),
            agent_revision: snapshot.revision,
            event_sequence: event.sequence,
            occurred_at: event.created_at,
            kind,
            snapshot,
            summary: None,
        });
    }

    pub(crate) fn publish_progress(
        &self,
        agent_id: &AgentId,
        kind: AgentUpdateKind,
        summary: Option<String>,
        signal_id: String,
    ) -> AgentRuntimeResult<()> {
        let snapshot = self.snapshot(agent_id)?;
        let Some(parent_agent_id) = snapshot.identity.parent_id.clone() else {
            return Ok(());
        };
        self.send(AgentUpdateEnvelope {
            signal_id,
            parent_agent_id,
            agent_id: agent_id.clone(),
            agent_revision: snapshot.revision,
            event_sequence: snapshot.event_sequence,
            occurred_at: snapshot.updated_at,
            kind,
            snapshot,
            summary,
        });
        Ok(())
    }

    pub(crate) fn suspend_parent_wait(&self, parent_agent_id: AgentId) {
        self.inner
            .quiesced_parent_waits
            .write()
            .expect("quiesced parent waits lock poisoned")
            .insert(parent_agent_id.clone());
        let _ = self.inner.parent_wait_sender.send(parent_agent_id);
    }

    pub(crate) fn resume_parent_wait(&self, parent_agent_id: &AgentId) {
        self.inner
            .quiesced_parent_waits
            .write()
            .expect("quiesced parent waits lock poisoned")
            .remove(parent_agent_id);
    }

    pub(crate) fn parent_wait_is_suspended(&self, parent_agent_id: &AgentId) -> bool {
        self.inner
            .quiesced_parent_waits
            .read()
            .expect("quiesced parent waits lock poisoned")
            .contains(parent_agent_id)
    }

    pub(crate) fn subscribe_parent_wait_controls(&self) -> broadcast::Receiver<AgentId> {
        self.inner.parent_wait_sender.subscribe()
    }

    pub(crate) fn publish_product_phase(
        &self,
        parent_agent_id: AgentId,
        agent_id: AgentId,
        signal_id: String,
        phase: String,
        summary: Option<String>,
    ) -> AgentRuntimeResult<()> {
        let snapshot = self.snapshot(&agent_id)?;
        if snapshot.identity.parent_id.as_ref() != Some(&parent_agent_id) {
            return Err(AgentRuntimeError::NotFound(agent_id));
        }
        self.send(AgentUpdateEnvelope {
            signal_id,
            parent_agent_id,
            agent_id,
            agent_revision: snapshot.revision,
            event_sequence: snapshot.event_sequence,
            occurred_at: snapshot.updated_at,
            kind: AgentUpdateKind::ProductPhaseChanged { phase },
            snapshot,
            summary,
        });
        Ok(())
    }

    pub(crate) fn store_snapshot(&self, snapshot: AgentSnapshot) {
        self.inner
            .snapshots
            .write()
            .expect("agent event snapshots lock poisoned")
            .insert(snapshot.identity.id.clone(), snapshot.clone());
        let _ = self.inner.snapshot_sender.send(snapshot);
    }

    pub(crate) fn snapshot(&self, agent_id: &AgentId) -> AgentRuntimeResult<AgentSnapshot> {
        self.inner
            .snapshots
            .read()
            .expect("agent event snapshots lock poisoned")
            .get(agent_id)
            .cloned()
            .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))
    }

    pub(crate) fn children(&self, parent_agent_id: &AgentId) -> Vec<AgentSnapshot> {
        self.inner
            .snapshots
            .read()
            .expect("agent event snapshots lock poisoned")
            .values()
            .filter(|snapshot| snapshot.identity.parent_id.as_ref() == Some(parent_agent_id))
            .cloned()
            .collect()
    }

    pub(crate) fn snapshots(&self) -> Vec<AgentSnapshot> {
        self.inner
            .snapshots
            .read()
            .expect("agent event snapshots lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub(crate) fn subscribe_parent(&self, parent_agent_id: &AgentId) -> AgentParentSubscription {
        let receiver = self
            .inner
            .parent_senders
            .write()
            .expect("agent parent senders lock poisoned")
            .entry(parent_agent_id.clone())
            .or_insert_with(|| broadcast::channel(EVENT_CAPACITY).0)
            .subscribe();
        let children = self.children(parent_agent_id);
        AgentParentSubscription {
            through_sequence: children
                .iter()
                .map(|snapshot| snapshot.event_sequence)
                .max()
                .unwrap_or_default(),
            children,
            receiver,
        }
    }

    pub(crate) fn subscribe_all(&self) -> broadcast::Receiver<AgentUpdateEnvelope> {
        self.inner.sender.subscribe()
    }

    pub(crate) fn subscribe_runtime(&self) -> broadcast::Receiver<AgentRuntimeEvent> {
        self.inner.runtime_sender.subscribe()
    }

    pub(crate) fn subscribe_snapshots(&self) -> broadcast::Receiver<AgentSnapshot> {
        self.inner.snapshot_sender.subscribe()
    }

    fn send(&self, mut update: AgentUpdateEnvelope) {
        update.summary = update.summary.map(bound_summary);
        let _ = self.inner.sender.send(update.clone());
        if let Some(sender) = self
            .inner
            .parent_senders
            .read()
            .expect("agent parent senders lock poisoned")
            .get(&update.parent_agent_id)
        {
            let _ = sender.send(update);
        }
    }
}

fn bound_summary(summary: String) -> String {
    let Some((byte_index, _)) = summary.char_indices().nth(MAX_SUMMARY_CHARS) else {
        return summary;
    };
    summary[..byte_index].to_string()
}

fn snapshot_for_event(kind: &AgentRuntimeEventKind) -> AgentSnapshot {
    match kind {
        AgentRuntimeEventKind::Registered { snapshot }
        | AgentRuntimeEventKind::StateChanged { snapshot }
        | AgentRuntimeEventKind::TurnQueued { snapshot, .. }
        | AgentRuntimeEventKind::TurnStarted { snapshot, .. }
        | AgentRuntimeEventKind::SessionOpened { snapshot, .. }
        | AgentRuntimeEventKind::TurnFinished { snapshot, .. }
        | AgentRuntimeEventKind::RecoveryCancelledTurn { snapshot, .. }
        | AgentRuntimeEventKind::Faulted { snapshot, .. } => snapshot.clone(),
    }
}

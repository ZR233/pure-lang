use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use tokio::sync::watch;

use super::{
    AgentRuntimeError, AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeResult, AgentSnapshot,
    ThreadId,
};

/// Agent Directory 的 canonical 快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDirectorySnapshot {
    pub revision: u64,
    pub agents: Vec<AgentSnapshot>,
}

/// Agent Directory 的单一 revision 订阅。
pub struct AgentDirectorySubscription {
    baseline_revision: u64,
    receiver: watch::Receiver<u64>,
}

impl AgentDirectorySubscription {
    pub fn baseline_revision(&self) -> u64 {
        self.baseline_revision
    }

    pub async fn changed(&mut self) -> AgentRuntimeResult<u64> {
        self.receiver
            .changed()
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)?;
        Ok(*self.receiver.borrow_and_update())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentDirectoryHandle {
    inner: Arc<AgentDirectoryInner>,
}

#[derive(Debug)]
struct AgentDirectoryInner {
    snapshots: RwLock<BTreeMap<ThreadId, AgentSnapshot>>,
    revision: AtomicU64,
    revision_sender: watch::Sender<u64>,
    history_readers: RwLock<BTreeMap<u64, HistoryReader>>,
    next_reader: AtomicU64,
}

#[derive(Debug)]
struct HistoryReader {
    caller: ThreadId,
    selector: super::AgentTargetSelector,
    released: BTreeSet<ThreadId>,
}

/// Retains canonical target history until a collaboration source has observed closure.
pub(crate) struct AgentHistoryLease {
    directory: Weak<AgentDirectoryInner>,
    id: u64,
}

impl AgentHistoryLease {
    pub(crate) fn acknowledge_closed(&self, id: &ThreadId) {
        if let Some(directory) = self.directory.upgrade()
            && let Some(reader) = directory
                .history_readers
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_mut(&self.id)
        {
            reader.released.insert(id.clone());
        }
    }
}

impl Drop for AgentHistoryLease {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.upgrade() {
            directory
                .history_readers
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.id);
        }
    }
}

impl AgentDirectoryHandle {
    pub(crate) fn new(restored: impl IntoIterator<Item = AgentSnapshot>) -> Self {
        let snapshots = restored
            .into_iter()
            .map(|snapshot| (snapshot.identity.id.clone(), snapshot))
            .collect();
        let (revision_sender, _) = watch::channel(1);
        Self {
            inner: Arc::new(AgentDirectoryInner {
                snapshots: RwLock::new(snapshots),
                revision: AtomicU64::new(1),
                revision_sender,
                history_readers: RwLock::new(BTreeMap::new()),
                next_reader: AtomicU64::new(1),
            }),
        }
    }

    /// 在 durable commit 成功后更新 snapshot；只有协作可见事实推进 watch。
    pub(crate) fn publish_runtime_event(&self, event: &AgentRuntimeEvent) {
        let snapshot = snapshot_for_event(&event.kind);
        let previous = self.snapshot(&snapshot.identity.id).ok();
        if previous.as_ref().is_some_and(|previous| {
            previous.revision > snapshot.revision
                || (previous.revision == snapshot.revision
                    && previous.event_sequence >= snapshot.event_sequence)
        }) {
            return;
        }
        self.store_snapshot(snapshot.clone());
        if directory_fact_changed(previous.as_ref(), &snapshot, &event.kind) {
            self.advance_revision();
        }
    }

    pub(crate) fn store_snapshot(&self, snapshot: AgentSnapshot) {
        if !matches!(snapshot.state, super::AgentState::Closed(_)) {
            for reader in self
                .inner
                .history_readers
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values_mut()
            {
                reader.released.remove(&snapshot.identity.id);
            }
        }
        self.inner
            .snapshots
            .write()
            .expect("agent directory snapshots lock poisoned")
            .insert(snapshot.identity.id.clone(), snapshot);
    }

    pub(crate) fn retain_history(
        &self,
        caller: ThreadId,
        selector: super::AgentTargetSelector,
    ) -> AgentRuntimeResult<AgentHistoryLease> {
        let id = self
            .inner
            .next_reader
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| {
                AgentRuntimeError::Lifecycle("agent history lease IDs exhausted".into())
            })?;
        self.inner
            .history_readers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                id,
                HistoryReader {
                    caller,
                    selector,
                    released: BTreeSet::new(),
                },
            );
        Ok(AgentHistoryLease {
            directory: Arc::downgrade(&self.inner),
            id,
        })
    }

    pub(crate) fn history_is_retained(&self, id: &ThreadId) -> bool {
        let snapshots = self.snapshots();
        self.inner
            .history_readers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .any(|reader| {
                reader.caller != *id
                    && !reader.released.contains(id)
                    && super::collaboration::support::filter_visible(
                        &snapshots,
                        &reader.caller,
                        &reader.selector,
                    )
                    .iter()
                    .any(|snapshot| snapshot.identity.id == *id)
            })
    }

    /// LRU 淘汰驻留 actor 时移除其 directory 条目；返回是否存在。
    pub(crate) fn remove(&self, agent_id: &ThreadId) -> bool {
        let removed = self
            .inner
            .snapshots
            .write()
            .expect("agent directory snapshots lock poisoned")
            .remove(agent_id)
            .is_some();
        if removed {
            self.advance_revision();
        }
        removed
    }

    pub(crate) fn snapshot(&self, agent_id: &ThreadId) -> AgentRuntimeResult<AgentSnapshot> {
        self.inner
            .snapshots
            .read()
            .expect("agent directory snapshots lock poisoned")
            .get(agent_id)
            .cloned()
            .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))
    }

    pub(crate) fn snapshots(&self) -> Vec<AgentSnapshot> {
        let mut snapshots = self
            .inner
            .snapshots
            .read()
            .expect("agent directory snapshots lock poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.identity.id.cmp(&right.identity.id));
        snapshots
    }

    pub(crate) fn directory_snapshot(&self) -> AgentDirectorySnapshot {
        AgentDirectorySnapshot {
            revision: self.inner.revision.load(Ordering::Acquire),
            agents: self.snapshots(),
        }
    }

    pub(crate) fn subscribe(&self) -> AgentDirectorySubscription {
        AgentDirectorySubscription {
            baseline_revision: self.inner.revision.load(Ordering::Acquire),
            receiver: self.inner.revision_sender.subscribe(),
        }
    }

    fn advance_revision(&self) {
        let revision = self
            .inner
            .revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.inner.revision_sender.send_replace(revision);
    }
}

pub(crate) fn root_agent_id_for(
    directory: &AgentDirectorySnapshot,
    agent_id: &ThreadId,
) -> AgentRuntimeResult<ThreadId> {
    let parents = directory
        .agents
        .iter()
        .map(|snapshot| {
            (
                snapshot.identity.id.clone(),
                snapshot.identity.parent_id.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if !parents.contains_key(agent_id) {
        return Err(AgentRuntimeError::NotFound(agent_id.clone()));
    }
    let mut current = agent_id.clone();
    let mut remaining = parents.len();
    while let Some(parent) = parents.get(&current).cloned().flatten() {
        if remaining == 0 {
            return Err(AgentRuntimeError::Lifecycle(
                "agent parent graph contains a cycle".to_string(),
            ));
        }
        remaining -= 1;
        current = parent;
        if !parents.contains_key(&current) {
            return Err(AgentRuntimeError::Lifecycle(format!(
                "agent parent {} is missing while resolving root for {agent_id}",
                current.as_str()
            )));
        }
    }
    Ok(current)
}

fn directory_fact_changed(
    previous: Option<&AgentSnapshot>,
    snapshot: &AgentSnapshot,
    kind: &AgentRuntimeEventKind,
) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    if previous.progress != snapshot.progress
        || (previous.state != snapshot.state && snapshot.state.is_waiting_interaction())
    {
        return true;
    }
    matches!(
        kind,
        AgentRuntimeEventKind::TurnFinished { .. }
            | AgentRuntimeEventKind::RecoveryCancelledTurn { .. }
            | AgentRuntimeEventKind::Faulted { .. }
    ) || !snapshot.state.is_operational()
}

fn snapshot_for_event(kind: &AgentRuntimeEventKind) -> AgentSnapshot {
    match kind {
        AgentRuntimeEventKind::Registered { snapshot }
        | AgentRuntimeEventKind::StateChanged { snapshot }
        | AgentRuntimeEventKind::TurnQueued { snapshot, .. }
        | AgentRuntimeEventKind::TurnStarted { snapshot, .. }
        | AgentRuntimeEventKind::ThreadOpened { snapshot, .. }
        | AgentRuntimeEventKind::TurnActivityChanged { snapshot, .. }
        | AgentRuntimeEventKind::TurnFinished { snapshot, .. }
        | AgentRuntimeEventKind::RecoveryCancelledTurn { snapshot, .. }
        | AgentRuntimeEventKind::Faulted { snapshot, .. } => snapshot.as_ref().clone(),
    }
}

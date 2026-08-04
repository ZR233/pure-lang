use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use pl_protocol::{
    InteractionStatus, ThreadItem, ThreadItemContent, ThreadItemDelta, ThreadItemDeltaField,
    ThreadNotification, ThreadNotificationEnvelope, ThreadSnapshot, ThreadSubscriptionRequest,
    ThreadSubscriptionUpdate,
};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

mod fact;
mod observation;
mod projector;

pub use fact::ThreadNotificationFact;
pub(crate) use fact::project_thread_facts;
pub(crate) use observation::{
    ObservedTurnEvent, TurnObservation, compaction_observation, observation_from_agent_event,
    project_observation,
};
pub(crate) use projector::{project_runtime_event, project_trace_events, runtime_event_thread_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadEventOptions {
    pub channel_capacity: usize,
}

impl Default for ThreadEventOptions {
    fn default() -> Self {
        Self {
            channel_capacity: 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadEventError {
    ThreadNotFound(String),
    ThreadMismatch {
        expected: String,
        actual: String,
    },
    RevisionGap {
        expected: u64,
        actual: u64,
    },
    ItemRevisionGap {
        item_id: String,
        expected: u64,
        actual: u64,
    },
    ProjectionInvariant(String),
    LockPoisoned,
}

impl fmt::Display for ThreadEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThreadNotFound(thread_id) => {
                write!(formatter, "thread event channel not found: {thread_id}")
            }
            Self::ThreadMismatch { expected, actual } => write!(
                formatter,
                "thread notification targets {actual}, expected {expected}"
            ),
            Self::RevisionGap { expected, actual } => {
                write!(
                    formatter,
                    "thread revision gap: expected {expected}, got {actual}"
                )
            }
            Self::ItemRevisionGap {
                item_id,
                expected,
                actual,
            } => write!(
                formatter,
                "item {item_id} revision gap: expected {expected}, got {actual}"
            ),
            Self::ProjectionInvariant(message) => {
                write!(formatter, "thread projection invariant failed: {message}")
            }
            Self::LockPoisoned => formatter.write_str("thread event state lock poisoned"),
        }
    }
}

impl std::error::Error for ThreadEventError {}

#[derive(Clone)]
pub struct ThreadEventBus {
    inner: Arc<BusInner>,
}

struct BusInner {
    options: ThreadEventOptions,
    threads: RwLock<BTreeMap<String, Arc<ThreadChannel>>>,
}

struct ThreadChannel {
    state: Mutex<ThreadChannelState>,
    publish_lock: AsyncMutex<()>,
    next_subscriber_id: AtomicU64,
    capacity: usize,
}

struct ThreadChannelState {
    snapshot: ThreadSnapshot,
    subscribers: BTreeMap<u64, ThreadSubscriber>,
}

struct ThreadSubscriber {
    sender: mpsc::Sender<ThreadSubscriptionUpdate>,
    pending_lag: u64,
}

impl ThreadEventBus {
    pub fn new(options: ThreadEventOptions) -> Self {
        Self {
            inner: Arc::new(BusInner {
                options,
                threads: RwLock::new(BTreeMap::new()),
            }),
        }
    }

    pub fn handle(&self) -> ThreadEventBusHandle {
        ThreadEventBusHandle { bus: self.clone() }
    }

    pub fn replace_snapshot(&self, snapshot: ThreadSnapshot) -> Result<(), ThreadEventError> {
        let channel = self.channel_or_create(&snapshot.thread.id)?;
        channel
            .state
            .lock()
            .map_err(|_| ThreadEventError::LockPoisoned)?
            .snapshot = snapshot;
        Ok(())
    }

    pub async fn publish(
        &self,
        notification: ThreadNotificationEnvelope,
    ) -> Result<(), ThreadEventError> {
        let channel = self.channel_or_create(&notification.thread_id)?;
        let _publish_guard = channel.publish_lock.lock().await;
        if is_lossless_notification(&notification.notification) {
            self.publish_lossless(&channel, notification).await
        } else {
            self.publish_best_effort(&channel, notification)
        }
    }

    pub async fn publish_batch(
        &self,
        notifications: Vec<ThreadNotificationEnvelope>,
    ) -> Result<(), ThreadEventError> {
        for notification in notifications {
            self.publish(notification).await?;
        }
        Ok(())
    }

    pub fn snapshot(&self, thread_id: &str) -> Result<ThreadSnapshot, ThreadEventError> {
        Ok(self
            .channel(thread_id)?
            .state
            .lock()
            .map_err(|_| ThreadEventError::LockPoisoned)?
            .snapshot
            .clone())
    }

    pub(crate) fn project(
        &self,
        thread_id: &str,
        notifications: &[ThreadNotificationEnvelope],
    ) -> Result<ThreadSnapshot, ThreadEventError> {
        let mut snapshot = self
            .channel_or_create(thread_id)?
            .state
            .lock()
            .map_err(|_| ThreadEventError::LockPoisoned)?
            .snapshot
            .clone();
        for notification in notifications {
            apply_notification(&mut snapshot, notification)?;
        }
        Ok(snapshot)
    }

    pub fn subscribe(
        &self,
        request: ThreadSubscriptionRequest,
    ) -> Result<ThreadEventSubscription, ThreadEventError> {
        let channel = self.channel(&request.thread_id)?;
        let subscriber_id = channel.next_subscriber_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel(channel.capacity);
        let snapshot = {
            let mut state = channel
                .state
                .lock()
                .map_err(|_| ThreadEventError::LockPoisoned)?;
            state.subscribers.insert(
                subscriber_id,
                ThreadSubscriber {
                    sender,
                    pending_lag: 0,
                },
            );
            state.snapshot.clone()
        };
        Ok(ThreadEventSubscription {
            thread_id: request.thread_id,
            bootstrap: VecDeque::from([ThreadSubscriptionUpdate::Snapshot {
                snapshot: Box::new(snapshot),
            }]),
            receiver,
        })
    }

    async fn publish_lossless(
        &self,
        channel: &ThreadChannel,
        notification: ThreadNotificationEnvelope,
    ) -> Result<(), ThreadEventError> {
        let deliveries = {
            let mut state = channel
                .state
                .lock()
                .map_err(|_| ThreadEventError::LockPoisoned)?;
            apply_notification(&mut state.snapshot, &notification)?;
            state
                .subscribers
                .iter_mut()
                .map(|(subscriber_id, subscriber)| {
                    let pending_lag = std::mem::take(&mut subscriber.pending_lag);
                    (*subscriber_id, subscriber.sender.clone(), pending_lag)
                })
                .collect::<Vec<_>>()
        };
        let mut closed = Vec::new();
        for (subscriber_id, sender, pending_lag) in deliveries {
            if pending_lag > 0
                && sender
                    .send(lagged_update(&notification, pending_lag))
                    .await
                    .is_err()
            {
                closed.push(subscriber_id);
                continue;
            }
            if sender
                .send(ThreadSubscriptionUpdate::Notification {
                    notification: Box::new(notification.clone()),
                })
                .await
                .is_err()
            {
                closed.push(subscriber_id);
            }
        }
        self.remove_closed_subscribers(channel, &closed)
    }

    fn publish_best_effort(
        &self,
        channel: &ThreadChannel,
        notification: ThreadNotificationEnvelope,
    ) -> Result<(), ThreadEventError> {
        let mut state = channel
            .state
            .lock()
            .map_err(|_| ThreadEventError::LockPoisoned)?;
        apply_notification(&mut state.snapshot, &notification)?;
        state.subscribers.retain(|_, subscriber| {
            if subscriber.pending_lag > 0 {
                match subscriber
                    .sender
                    .try_send(lagged_update(&notification, subscriber.pending_lag))
                {
                    Ok(()) => subscriber.pending_lag = 0,
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        subscriber.pending_lag = subscriber.pending_lag.saturating_add(1);
                        return true;
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => return false,
                }
            }
            match subscriber
                .sender
                .try_send(ThreadSubscriptionUpdate::Notification {
                    notification: Box::new(notification.clone()),
                }) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    subscriber.pending_lag = subscriber.pending_lag.saturating_add(1);
                    true
                }
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            }
        });
        Ok(())
    }

    fn remove_closed_subscribers(
        &self,
        channel: &ThreadChannel,
        closed: &[u64],
    ) -> Result<(), ThreadEventError> {
        if closed.is_empty() {
            return Ok(());
        }
        let mut state = channel
            .state
            .lock()
            .map_err(|_| ThreadEventError::LockPoisoned)?;
        for subscriber_id in closed {
            state.subscribers.remove(subscriber_id);
        }
        Ok(())
    }

    fn channel(&self, thread_id: &str) -> Result<Arc<ThreadChannel>, ThreadEventError> {
        self.inner
            .threads
            .read()
            .map_err(|_| ThreadEventError::LockPoisoned)?
            .get(thread_id)
            .cloned()
            .ok_or_else(|| ThreadEventError::ThreadNotFound(thread_id.to_string()))
    }

    fn channel_or_create(&self, thread_id: &str) -> Result<Arc<ThreadChannel>, ThreadEventError> {
        let mut threads = self
            .inner
            .threads
            .write()
            .map_err(|_| ThreadEventError::LockPoisoned)?;
        Ok(threads
            .entry(thread_id.to_string())
            .or_insert_with(|| {
                Arc::new(ThreadChannel {
                    state: Mutex::new(ThreadChannelState {
                        snapshot: ThreadSnapshot::empty(thread_id),
                        subscribers: BTreeMap::new(),
                    }),
                    publish_lock: AsyncMutex::new(()),
                    next_subscriber_id: AtomicU64::new(0),
                    capacity: self.inner.options.channel_capacity.max(1),
                })
            })
            .clone())
    }
}

fn is_lossless_notification(notification: &ThreadNotification) -> bool {
    !matches!(
        notification,
        ThreadNotification::TurnUpdated { .. } | ThreadNotification::ThreadRuntimeUpdated { .. }
    )
}

fn lagged_update(source: &ThreadNotificationEnvelope, dropped: u64) -> ThreadSubscriptionUpdate {
    ThreadSubscriptionUpdate::Notification {
        notification: Box::new(ThreadNotificationEnvelope {
            thread_id: source.thread_id.clone(),
            revision: 0,
            emitted_at: source.emitted_at,
            notification: ThreadNotification::Lagged { dropped },
        }),
    }
}

impl Default for ThreadEventBus {
    fn default() -> Self {
        Self::new(ThreadEventOptions::default())
    }
}

impl fmt::Debug for ThreadEventBus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadEventBus")
            .field("options", &self.inner.options)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct ThreadEventBusHandle {
    bus: ThreadEventBus,
}

impl ThreadEventBusHandle {
    pub fn replace_snapshot(&self, snapshot: ThreadSnapshot) -> Result<(), ThreadEventError> {
        self.bus.replace_snapshot(snapshot)
    }

    pub async fn publish_batch(
        &self,
        notifications: Vec<ThreadNotificationEnvelope>,
    ) -> Result<(), ThreadEventError> {
        self.bus.publish_batch(notifications).await
    }

    pub fn snapshot(&self, thread_id: &str) -> Result<ThreadSnapshot, ThreadEventError> {
        self.bus.snapshot(thread_id)
    }

    pub(crate) fn project(
        &self,
        thread_id: &str,
        notifications: &[ThreadNotificationEnvelope],
    ) -> Result<ThreadSnapshot, ThreadEventError> {
        self.bus.project(thread_id, notifications)
    }

    pub fn subscribe(
        &self,
        request: ThreadSubscriptionRequest,
    ) -> Result<ThreadEventSubscription, ThreadEventError> {
        self.bus.subscribe(request)
    }
}

pub struct ThreadEventSubscription {
    thread_id: String,
    bootstrap: VecDeque<ThreadSubscriptionUpdate>,
    receiver: mpsc::Receiver<ThreadSubscriptionUpdate>,
}

impl ThreadEventSubscription {
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub async fn recv(&mut self) -> Option<ThreadSubscriptionUpdate> {
        if let Some(update) = self.bootstrap.pop_front() {
            return Some(update);
        }
        self.receiver.recv().await
    }
}

impl fmt::Debug for ThreadEventSubscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ThreadEventSubscription")
            .field("thread_id", &self.thread_id)
            .field("bootstrap_updates", &self.bootstrap.len())
            .finish_non_exhaustive()
    }
}

fn apply_notification(
    snapshot: &mut ThreadSnapshot,
    envelope: &ThreadNotificationEnvelope,
) -> Result<(), ThreadEventError> {
    if snapshot.thread.id != envelope.thread_id {
        return Err(ThreadEventError::ThreadMismatch {
            expected: snapshot.thread.id.clone(),
            actual: envelope.thread_id.clone(),
        });
    }
    let expected = snapshot.revision.saturating_add(1);
    if envelope.revision != expected {
        return Err(ThreadEventError::RevisionGap {
            expected,
            actual: envelope.revision,
        });
    }
    match &envelope.notification {
        ThreadNotification::TurnStarted { turn } | ThreadNotification::TurnUpdated { turn } => {
            snapshot.active_turn = Some(turn.clone());
        }
        ThreadNotification::TurnCompleted { turn } => {
            if snapshot
                .active_turn
                .as_ref()
                .is_some_and(|active| active.id == turn.id)
            {
                snapshot.active_turn = None;
            }
        }
        ThreadNotification::ItemStarted { item } | ThreadNotification::ItemCompleted { item } => {
            upsert_item(&mut snapshot.items, item)
        }
        ThreadNotification::ItemDelta { delta } => apply_delta(&mut snapshot.items, delta)?,
        ThreadNotification::InteractionChanged { interaction } => {
            snapshot
                .interactions
                .retain(|candidate| candidate.interaction_id != interaction.interaction_id);
            if interaction.status == InteractionStatus::Pending {
                snapshot.interactions.push(interaction.as_ref().clone());
            }
        }
        ThreadNotification::ThreadRuntimeUpdated { runtime } => {
            snapshot.runtime = Some((**runtime).clone());
        }
        ThreadNotification::Lagged { .. } => return Ok(()),
    }
    snapshot.revision = envelope.revision;
    Ok(())
}

fn upsert_item(items: &mut Vec<ThreadItem>, replacement: &ThreadItem) {
    if let Some(existing) = items.iter_mut().find(|item| item.id == replacement.id) {
        *existing = replacement.clone();
    } else {
        items.push(replacement.clone());
        items.sort_by_key(|item| item.ordinal);
    }
}

fn apply_delta(items: &mut [ThreadItem], delta: &ThreadItemDelta) -> Result<(), ThreadEventError> {
    let item = items
        .iter_mut()
        .find(|item| item.id == delta.item_id)
        .ok_or_else(|| {
            ThreadEventError::ProjectionInvariant(format!(
                "delta targets missing item {}",
                delta.item_id
            ))
        })?;
    let expected = item.revision.saturating_add(1);
    if delta.revision != expected {
        return Err(ThreadEventError::ItemRevisionGap {
            item_id: delta.item_id.clone(),
            expected,
            actual: delta.revision,
        });
    }
    match (&mut item.content, delta.field) {
        (
            ThreadItemContent::UserMessage { text, .. }
            | ThreadItemContent::AgentMessage { text, .. },
            ThreadItemDeltaField::Text,
        ) => text.push_str(&delta.delta),
        (ThreadItemContent::Reasoning { summary, .. }, ThreadItemDeltaField::ReasoningSummary) => {
            append_chunk(summary, delta.chunk_index, &delta.delta)
        }
        (ThreadItemContent::Reasoning { content, .. }, ThreadItemDeltaField::ReasoningContent) => {
            append_chunk(content, delta.chunk_index, &delta.delta)
        }
        (ThreadItemContent::Plan { content }, ThreadItemDeltaField::PlanContent) => {
            content.push_str(&delta.delta);
        }
        (ThreadItemContent::ToolCall { tool }, ThreadItemDeltaField::ToolArguments) => {
            tool.arguments.push_str(&delta.delta);
        }
        (ThreadItemContent::ToolCall { tool }, ThreadItemDeltaField::ToolResult) => {
            tool.result.get_or_insert_default().push_str(&delta.delta);
        }
        (content, field) => {
            return Err(ThreadEventError::ProjectionInvariant(format!(
                "delta field {field:?} is incompatible with item {content:?}"
            )));
        }
    }
    item.revision = delta.revision;
    Ok(())
}

fn append_chunk(chunks: &mut Vec<String>, chunk_index: Option<u32>, delta: &str) {
    let index = chunk_index.unwrap_or_default() as usize;
    if chunks.len() <= index {
        chunks.resize(index + 1, String::new());
    }
    chunks[index].push_str(delta);
}

#[cfg(test)]
mod tests;

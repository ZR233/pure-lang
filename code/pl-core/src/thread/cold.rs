//! Nonblocking effect admission and explicit durability barriers for Thread persistence.
use super::*;
use futures::future::BoxFuture;
use std::{fmt, future::Future, sync::Arc};

/// One fixed persistence ticket. The checkpoint may only publish after the effect fence is durable.
#[derive(Debug, Clone)]
pub struct ThreadWrite {
    pub effect: Arc<ThreadEffectBatch>,
    pub checkpoint: ThreadCheckpoint,
}

/// Storage failure retains its typed source; it never rolls back a Thread commit.
#[derive(Debug, thiserror::Error)]
#[error("cold store failed: {source}")]
pub struct ColdStoreError {
    #[source]
    pub source: Box<dyn std::error::Error + Send + Sync>,
}

/// Retained queued bytes reported by a backend; completed but unsaved work may exceed limits.
#[derive(Debug, Clone, Default)]
pub struct StoragePressure {
    pub thread_bytes: u64,
    pub store_bytes: u64,
    pub error: Option<Arc<ColdStoreError>>,
}

/// One durable tool-task fact read back from the host store.
///
/// It is the authority a repeated read-only task query uses after the owner has released the
/// finished result from its transient window: the task identity/status come from the host's durable
/// call facts and `delivery` carries the full committed result body when the call reached a
/// terminal delivery. It is a read of durable facts, never a resident second history.
#[derive(Debug, Clone)]
pub struct DurableToolTask {
    pub task: super::task::TaskRecord,
    pub delivery: Option<ToolDelivery>,
}

/// Injected asynchronous sink. `admit` only queues immutable effect data and must not block.
pub trait ColdStore: Send + Sync + fmt::Debug + 'static {
    /// Reserves an observed presentation identity synchronously, before a coalescing preview
    /// can discard it. Backends without ordered presentation storage need not reserve anything.
    fn reserve_observed_item(
        &self,
        _thread_id: &str,
        _item_id: &str,
    ) -> Result<(), ColdStoreError> {
        Ok(())
    }
    /// Reports current queue pressure without blocking on IO.
    fn pressure(&self, _thread_id: &str) -> StoragePressure {
        StoragePressure::default()
    }
    /// Optional notification for a change in durable progress or fault state.
    ///
    /// A Thread waits for this signal at a storage safety point while still servicing its
    /// mailbox. Backends without a notification are polled at a bounded interval.
    fn subscribe_pressure(&self, _thread_id: &str) -> Option<tokio::sync::watch::Receiver<()>> {
        None
    }
    /// Accepts exactly this encoded effect batch; repeated admission is idempotent.
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError>;
    /// Waits for previously admitted records to become durable, retaining failed writes for retry.
    fn flush(
        &self,
        thread_id: &str,
        sequence: u64,
    ) -> impl Future<Output = Result<(), ColdStoreError>> + Send;
    /// Reads one finished tool task by its core task identity from the durable store.
    ///
    /// A backend that keeps no task facts returns `Ok(None)`. Returning a non-terminal record means
    /// the delivery is not durable yet; callers must not present that as a finished result.
    fn read_tool_task(
        &self,
        _thread_id: &str,
        _task_id: &str,
    ) -> impl Future<Output = Result<Option<DurableToolTask>, ColdStoreError>> + Send {
        async { Ok(None) }
    }
}
trait ErasedColdStore: Send + Sync + fmt::Debug {
    fn reserve_observed_item(&self, thread_id: &str, item_id: &str) -> Result<(), ColdStoreError>;
    fn pressure(&self, thread_id: &str) -> StoragePressure;
    fn subscribe_pressure(&self, thread_id: &str) -> Option<tokio::sync::watch::Receiver<()>>;
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError>;
    fn flush<'a>(
        &'a self,
        thread_id: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<(), ColdStoreError>>;
    fn read_tool_task<'a>(
        &'a self,
        thread_id: &'a str,
        task_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<DurableToolTask>, ColdStoreError>>;
}
impl<T: ColdStore> ErasedColdStore for T {
    fn reserve_observed_item(&self, thread_id: &str, item_id: &str) -> Result<(), ColdStoreError> {
        ColdStore::reserve_observed_item(self, thread_id, item_id)
    }
    fn pressure(&self, thread_id: &str) -> StoragePressure {
        ColdStore::pressure(self, thread_id)
    }
    fn subscribe_pressure(&self, thread_id: &str) -> Option<tokio::sync::watch::Receiver<()>> {
        ColdStore::subscribe_pressure(self, thread_id)
    }
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError> {
        ColdStore::admit(self, thread_id, write)
    }
    fn flush<'a>(
        &'a self,
        thread_id: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<(), ColdStoreError>> {
        Box::pin(ColdStore::flush(self, thread_id, sequence))
    }
    fn read_tool_task<'a>(
        &'a self,
        thread_id: &'a str,
        task_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<DurableToolTask>, ColdStoreError>> {
        Box::pin(ColdStore::read_tool_task(self, thread_id, task_id))
    }
}

/// Shared store handle; Thread ownership and mutable model/tool instances remain private.
#[derive(Debug, Clone)]
pub struct ColdStoreHandle(Arc<dyn ErasedColdStore>);
impl ColdStoreHandle {
    /// Erases one backend; independent Threads may share its asynchronous writer.
    pub fn new(store: impl ColdStore) -> Self {
        Self(Arc::new(store))
    }

    pub(crate) fn reserve_observed_item(
        &self,
        thread_id: &str,
        item_id: &str,
    ) -> Result<(), ColdStoreError> {
        self.0.reserve_observed_item(thread_id, item_id)
    }

    pub(crate) fn subscribe_pressure(
        &self,
        thread_id: &str,
    ) -> Option<tokio::sync::watch::Receiver<()>> {
        self.0.subscribe_pressure(thread_id)
    }

    pub(crate) fn pressure(&self, thread_id: &str) -> StoragePressure {
        self.0.pressure(thread_id)
    }

    /// Reads one finished tool task from the durable store, if the backend keeps task facts.
    pub(crate) async fn read_tool_task(
        &self,
        thread_id: &str,
        task_id: &str,
    ) -> Result<Option<DurableToolTask>, ColdStoreError> {
        self.0.read_tool_task(thread_id, task_id).await
    }
}

/// Persistence progress is observable separately from authoritative runtime facts.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceState {
    pub attached: bool,
    pub pressure_paused: bool,
    pub pressure_warning: bool,
    pub pending_thread_bytes: u64,
    pub pending_store_bytes: u64,
    pub admitted_sequence: u64,
    pub durable_sequence: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub execution_phase: StorageExecutionPhase,
}

/// A storage pause is not a failed or cancelled Turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageExecutionPhase {
    #[default]
    Running,
    PausingForStorage,
    PausedForStorage,
}

impl Owner {
    /// Wait at an execution safety point without failing the active Turn or starving control.
    pub(super) async fn await_storage_admission(
        &mut self,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<(), ThreadError> {
        let mut updates = self
            .cold
            .as_ref()
            .and_then(|cold| cold.subscribe_pressure(&self.id));
        loop {
            self.admit_cold();
            self.refresh_storage_pressure();
            if !self.state.persistence.pressure_paused && self.cold_error.is_none() {
                if self.state.persistence.execution_phase != StorageExecutionPhase::Running {
                    self.state.persistence.execution_phase = StorageExecutionPhase::Running;
                    self.publish_snapshot();
                }
                return Ok(());
            }
            if self.state.persistence.execution_phase != StorageExecutionPhase::PausedForStorage {
                self.state.persistence.execution_phase = StorageExecutionPhase::PausingForStorage;
                self.publish_snapshot();
                self.state.persistence.execution_phase = StorageExecutionPhase::PausedForStorage;
                self.publish_snapshot();
            }
            let cancelled = self
                .await_with_mailbox(async {
                    tokio::select! {
                        () = cancellation.cancelled() => true,
                        available = async {
                            match updates.as_mut() {
                                Some(update) => update.changed().await.is_ok(),
                                None => std::future::pending::<bool>().await,
                            }
                        } => { if !available { updates = None; } false },
                        () = tokio::time::sleep(std::time::Duration::from_secs(1)) => false,
                    }
                })
                .await;
            if cancelled || self.interrupt.is_closing() {
                return Err(ThreadError::Cancelled);
            }
        }
    }

    pub(super) fn refresh_storage_pressure(&mut self) {
        let pressure = match &self.cold {
            Some(store) => store.0.pressure(&self.id),
            // Without an attached store every published batch stays unadmitted below, so the same
            // byte budget still gates admission. The accepted facts are never dropped to make room
            // for new work; the Thread reports pressure and pauses further admission instead of
            // letting a transient window grow without bound.
            None => StoragePressure::default(),
        };
        // A synchronous `admit` rejection is a fact about *these* effects: they were published but
        // the store did not take them, so a healthy pressure report from a background writer must
        // not erase it. Otherwise the Thread would keep executing model work whose facts are not
        // handed over — and a rejected attempt could be silently corrected instead of failing closed.
        // Only when every published effect really was handed over does a healthy report release a
        // latch that only reflected a transient background conflict.
        let handed_over = match self.pending_effects.front() {
            Some(pending) => {
                pending.write.effect.sequence <= self.state.persistence.admitted_sequence
            }
            None => true,
        };
        match &pressure.error {
            Some(error) => {
                self.state.persistence.error = Some(error.to_string());
                self.cold_error = Some(error.clone());
            }
            None if handed_over => {
                self.state.persistence.error = None;
                self.cold_error = None;
            }
            None => {}
        }
        // A queue drained by an attached store costs nothing here; only a queue that really holds
        // undurable batches is measured, and each of those batches is serialized at most once.
        let unadmitted = if self.pending_effects.is_empty() {
            0
        } else {
            self.pending_effects
                .iter_mut()
                .fold(0_u64, |total, pending| {
                    let bytes = match pending.encoded_bytes {
                        Some(bytes) => bytes,
                        None => {
                            // An unencodable batch keeps the pessimistic `u64::MAX` weight, exactly
                            // like the previous fold-based accounting did.
                            let bytes = pending
                                .write
                                .effect
                                .encode()
                                .map_or(u64::MAX, |payload| payload.content().len() as u64);
                            pending.encoded_bytes = Some(bytes);
                            bytes
                        }
                    };
                    total.saturating_add(bytes)
                })
        };
        let thread = pressure.thread_bytes.saturating_add(unadmitted);
        let total = pressure.store_bytes.saturating_add(unadmitted);
        let state = &mut self.state.persistence;
        state.pending_thread_bytes = thread;
        state.pending_store_bytes = total;
        if thread >= 64 * 1024 * 1024 || total >= 256 * 1024 * 1024 {
            state.pressure_paused = true;
        } else if thread <= 32 * 1024 * 1024 && total <= 128 * 1024 * 1024 {
            state.pressure_paused = false;
        }
        state.pressure_warning = thread >= 32 * 1024 * 1024 || total >= 128 * 1024 * 1024;
    }

    pub(super) fn admit_cold(&mut self) {
        let Some(store) = &self.cold else {
            return;
        };
        self.state.persistence.attached = true;
        while let Some(pending) = self.pending_effects.front() {
            let sequence = pending.write.effect.sequence;
            match store.0.admit(&self.id, pending.write.clone()) {
                Ok(()) => {
                    self.state.persistence.admitted_sequence = sequence;
                    self.pending_effects.pop_front();
                }
                Err(error) => {
                    self.state.persistence.error = Some(error.to_string());
                    self.cold_error = Some(Arc::new(error));
                    return;
                }
            }
        }
        self.state.persistence.error = None;
        self.cold_error = None;
    }

    pub(super) async fn flush_cold(&mut self) -> Result<(), ThreadError> {
        self.admit_cold();
        if let Some(error) = &self.cold_error {
            self.publish_snapshot();
            return Err(ThreadError::Storage(error.clone()));
        }
        let Some(store) = &self.cold else {
            return Ok(());
        };
        let result = store
            .0
            .flush(&self.id, self.state.persistence.admitted_sequence)
            .await;
        match result {
            Ok(()) => {
                self.state.persistence.durable_sequence = self.state.persistence.admitted_sequence;
                // The durable handoff makes every covered commit recoverable from history, so the
                // live window may release those bodies once its byte budget needs the space.
                self.effect_window
                    .release_through(self.state.persistence.durable_sequence);
            }
            Err(error) => {
                // A caller that asked for a fixed durable target and did not get it has a terminal
                // fact: answer with it directly instead of letting the pressure mirror below
                // mistake the store's momentary report for recovery.
                let error = Arc::new(error);
                self.state.persistence.error = Some(error.to_string());
                self.cold_error = Some(error.clone());
                self.publish_snapshot();
                return Err(ThreadError::Storage(error));
            }
        }
        self.refresh_storage_pressure();
        self.publish_snapshot();
        match &self.cold_error {
            Some(error) => Err(ThreadError::Storage(error.clone())),
            None => Ok(()),
        }
    }
}

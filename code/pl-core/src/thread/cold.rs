//! Nonblocking commit admission and explicit durability barriers for Thread journals.
use super::*;
use futures::future::BoxFuture;
use std::{fmt, future::Future, sync::Arc};

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

/// Injected asynchronous store. `admit` only queues immutable data and must not perform blocking IO.
pub trait ColdStore: Send + Sync + fmt::Debug + 'static {
    /// Reports current queue pressure without blocking on IO.
    fn pressure(&self, _thread_id: &str) -> StoragePressure {
        StoragePressure::default()
    }
    /// Accepts exactly these encoded bytes; repeated admission of the same identity is idempotent.
    fn admit(
        &self,
        thread_id: &str,
        sequence: u64,
        payload: OpaquePayload,
    ) -> Result<(), ColdStoreError>;
    /// Waits for previously admitted records to become durable, retaining failed writes for retry.
    fn flush(
        &self,
        thread_id: &str,
        sequence: u64,
    ) -> impl Future<Output = Result<(), ColdStoreError>> + Send;
}
trait ErasedColdStore: Send + Sync + fmt::Debug {
    fn pressure(&self, thread_id: &str) -> StoragePressure;
    fn admit(
        &self,
        thread_id: &str,
        sequence: u64,
        payload: OpaquePayload,
    ) -> Result<(), ColdStoreError>;
    fn flush<'a>(
        &'a self,
        thread_id: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<(), ColdStoreError>>;
}
impl<T: ColdStore> ErasedColdStore for T {
    fn pressure(&self, thread_id: &str) -> StoragePressure {
        ColdStore::pressure(self, thread_id)
    }
    fn admit(
        &self,
        thread_id: &str,
        sequence: u64,
        payload: OpaquePayload,
    ) -> Result<(), ColdStoreError> {
        ColdStore::admit(self, thread_id, sequence, payload)
    }
    fn flush<'a>(
        &'a self,
        thread_id: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<(), ColdStoreError>> {
        Box::pin(ColdStore::flush(self, thread_id, sequence))
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
}

impl Owner {
    pub(super) fn refresh_storage_pressure(&mut self) {
        let Some(store) = &self.cold else {
            return;
        };
        let pressure = store.0.pressure(&self.id);
        if let Some(error) = pressure.error {
            self.state.persistence.error = Some(error.to_string());
            self.cold_error = Some(error);
        }
        let unadmitted = self
            .encoded_journal
            .iter()
            .skip(self.state.persistence.admitted_sequence as usize)
            .fold(0_u64, |total, payload| {
                total.saturating_add(
                    payload
                        .as_ref()
                        .map_or(u64::MAX, |payload| payload.content().len() as u64),
                )
            });
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
        for (commit, encoding) in self
            .journal
            .iter()
            .zip(&self.encoded_journal)
            .skip(self.state.persistence.admitted_sequence as usize)
        {
            let encoded = match encoding {
                Ok(encoded) => encoded.clone(),
                Err(error) => {
                    let error = Arc::new(ColdStoreError {
                        source: Box::new(error.clone()),
                    });
                    self.state.persistence.error = Some(error.to_string());
                    self.cold_error = Some(error);
                    return;
                }
            };
            match store.0.admit(&self.id, commit.sequence, encoded) {
                Ok(()) => self.state.persistence.admitted_sequence = commit.sequence,
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
                self.state.persistence.durable_sequence = self.state.persistence.admitted_sequence
            }
            Err(error) => {
                self.state.persistence.error = Some(error.to_string());
                self.cold_error = Some(Arc::new(error));
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

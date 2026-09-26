//! SQLite cold-store adapter for protocol-independent Thread commit payloads.
use super::SqliteSessionStore;
use crate::thread::cold::{ColdStore, ColdStoreError, ThreadWrite};

/// Typed category of a plain SQLite store failure.
///
/// The category comes from the error variant the writer already recorded, never from its message, so
/// the owner's storage latch cannot be moved by error text.
fn storage_fault_kind(error: &super::SessionStoreError) -> crate::thread::cold::StorageFaultKind {
    use super::SessionStoreError;
    use crate::thread::cold::StorageFaultKind;
    match error {
        SessionStoreError::Stopped | SessionStoreError::Panicked => {
            StorageFaultKind::WriterUnavailable
        }
        // A rejected write is reported by its variant alone. Transient queue backpressure is the
        // owner's own byte budget (`QueueFull` from the reliable queue), not a message scrape.
        SessionStoreError::Conflict { .. }
        | SessionStoreError::Database(_)
        | SessionStoreError::Io(_)
        | SessionStoreError::Codec(_)
        | SessionStoreError::Replay(_)
        | SessionStoreError::Invalid(_)
        | SessionStoreError::UnsupportedSchema { .. }
        | SessionStoreError::InitializationCleanup { .. } => StorageFaultKind::WriteFailed,
    }
}

impl ColdStore for SqliteSessionStore {
    fn pressure(&self, thread_id: &str) -> crate::thread::cold::StoragePressure {
        let (thread_bytes, store_bytes) = self.pending_bytes(thread_id);
        crate::thread::cold::StoragePressure {
            thread_bytes,
            store_bytes,
            // Typed durability receipt: the plain SQLite backend reports how far this Thread's
            // commits are saved, so its owner releases the live effect window on a normal save
            // exactly like the product backend does.
            durable_sequence: self.thread_durable_sequence(thread_id),
            // Typed fault category: the writer already holds a typed store failure, so the owner
            // mirrors a category instead of parsing the error text. This backend has no per-fault
            // generation (that is the Studio coordinator's recovery ticket), so it reports `0`.
            fault: self.persistence().error.as_deref().map(storage_fault_kind),
            fault_generation: 0,
            // Every fault this backend names is generation `0`, so its own recovery receipt is
            // "generation 0 is healthy again": the writer retries inside this store, and once it
            // holds no error the one generation it ever reports is recovered. Without this the
            // owner would wait for a verdict a generation-less backend can never address.
            recovered_generation: self.persistence().error.is_none().then_some(0),
            error: self.persistence().error.map(|source| {
                std::sync::Arc::new(ColdStoreError {
                    source: Box::new(source),
                })
            }),
        }
    }

    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError> {
        let sequence = write.effect.sequence;
        let payload = write.effect.encode().map_err(|source| ColdStoreError {
            source: Box::new(source),
        })?;
        self.register_immutable_payload(
            thread_id,
            &format!("thread-commit.{sequence:020}"),
            payload,
        )
        .map_err(|source| ColdStoreError {
            source: Box::new(source),
        })
    }
    async fn flush(&self, thread_id: &str, sequence: u64) -> Result<(), ColdStoreError> {
        if sequence == 0 {
            return Ok(());
        }
        self.retry();
        self.flush_resource(
            thread_id,
            &format!("pl.resource.thread-commit.{sequence:020}"),
        )
        .await
        .map_err(|source| ColdStoreError {
            source: Box::new(source),
        })
    }
}

impl SqliteSessionStore {
    /// Reads validated legacy commit facts for one-way old-data migration only.
    ///
    /// # Errors
    /// Rejects corrupt storage envelopes, mismatched Thread identity or noncontiguous commits.
    #[doc(hidden)]
    pub async fn read_legacy_thread_journal(
        &self,
        thread_id: &str,
    ) -> Result<Vec<std::sync::Arc<crate::thread::ThreadEffectBatch>>, super::SessionStoreError>
    {
        let mut records = self
            .replay_entries(thread_id, None)
            .await?
            .into_iter()
            .filter(|entry| entry.id.starts_with("pl.resource.thread-commit."))
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        let commits = records
            .into_iter()
            .map(|entry| {
                let payload = crate::context::OpaquePayload::new(
                    entry.type_id,
                    entry.schema_version,
                    entry.payload,
                )
                .map_err(|error| super::SessionStoreError::Invalid(error.to_string()))?;
                let commit = crate::thread::ThreadEffectBatch::decode(&payload)
                    .map_err(|error| super::SessionStoreError::Invalid(error.to_string()))?;
                if commit.thread_id != thread_id
                    || entry.id != format!("pl.resource.thread-commit.{:020}", commit.sequence)
                {
                    return Err(super::SessionStoreError::Invalid(
                        "Thread commit ownership or sequence key mismatch".into(),
                    ));
                }
                Ok(std::sync::Arc::new(commit))
            })
            .collect::<Result<Vec<_>, _>>()?;
        crate::thread::journal::legacy_migration::replay(&commits)
            .map_err(|error| super::SessionStoreError::Invalid(error.to_string()))?;
        Ok(commits)
    }
}

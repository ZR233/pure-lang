//! SQLite cold-store adapter for protocol-independent Thread commit payloads.
use super::SqliteSessionStore;
use crate::thread::cold::{ColdStore, ColdStoreError};

impl ColdStore for SqliteSessionStore {
    fn pressure(&self, thread_id: &str) -> crate::thread::cold::StoragePressure {
        let (thread_bytes, store_bytes) = self.pending_bytes(thread_id);
        crate::thread::cold::StoragePressure {
            thread_bytes,
            store_bytes,
            error: self.persistence().error.map(|source| {
                std::sync::Arc::new(ColdStoreError {
                    source: Box::new(source),
                })
            }),
        }
    }

    fn admit(
        &self,
        thread_id: &str,
        sequence: u64,
        payload: crate::context::OpaquePayload,
    ) -> Result<(), ColdStoreError> {
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
    /// Reads encoded Thread commits without model/tool activation or business payload decoding.
    ///
    /// # Errors
    /// Returns corrupt outer commit encoding or invalid sequence/context relationships.
    pub async fn replay_thread(
        &self,
        thread_id: &str,
    ) -> Result<crate::thread::ThreadSnapshot, super::SessionStoreError> {
        let commits = self.read_thread_journal(thread_id).await?;
        crate::thread::journal::replay(&commits)
            .map_err(|error| super::SessionStoreError::Invalid(error.to_string()))
    }

    /// Reads validated commit facts for cold restoration into fresh model and tool instances.
    ///
    /// # Errors
    /// Rejects corrupt storage envelopes, mismatched Thread identity or noncontiguous commits.
    pub async fn read_thread_journal(
        &self,
        thread_id: &str,
    ) -> Result<Vec<std::sync::Arc<crate::thread::journal::ThreadCommit>>, super::SessionStoreError>
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
                let commit = crate::thread::journal::ThreadCommit::decode(&payload)
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
        crate::thread::journal::replay(&commits)
            .map_err(|error| super::SessionStoreError::Invalid(error.to_string()))?;
        Ok(commits)
    }
}

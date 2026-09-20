//! SQLite cold-store adapter for protocol-independent Thread commit payloads.
use super::SqliteSessionStore;
use crate::thread::cold::{ColdStore, ColdStoreError};

/// Reserved immutable-resource id prefix for encoded Thread commits.
const COMMIT_RESOURCE_PREFIX: &str = "pl.resource.thread-commit.";

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
            .filter(|entry| entry.id.starts_with(COMMIT_RESOURCE_PREFIX))
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        let commits = records
            .into_iter()
            .map(|entry| decode_thread_commit(thread_id, entry))
            .collect::<Result<Vec<_>, _>>()?;
        crate::thread::journal::replay(&commits)
            .map_err(|error| super::SessionStoreError::Invalid(error.to_string()))?;
        Ok(commits)
    }

    /// Ordered, bounded keyset read of Thread commits for derived-index stages.
    ///
    /// Returns commits whose one-based `sequence` is strictly greater than
    /// `after_sequence`, in ascending sequence order, at most `limit` records.
    /// It never replays the whole journal prefix and never uses a deep OFFSET,
    /// so a Studio-derived index can advance incrementally against the same
    /// canonical journal that owns these facts.
    ///
    /// # Errors
    /// Rejects corrupt envelopes, a broken `pl.resource.thread-commit.<sequence>`
    /// key or a commit whose Thread ownership differs from `thread_id`.
    pub async fn read_thread_journal_page(
        &self,
        thread_id: &str,
        after_sequence: u64,
        limit: std::num::NonZeroUsize,
    ) -> Result<Vec<std::sync::Arc<crate::thread::journal::ThreadCommit>>, super::SessionStoreError>
    {
        read_commit_page(&self.owner.shared.db, thread_id, after_sequence, limit).await
    }
}

/// Read-only, non-hydrating view over one existing session database's Thread journal.
///
/// It starts no writer and hydrates no resource map; it only reads the generic commit
/// envelopes in bounded keyset pages, so Studio cold reads and derived-index building
/// never pull a whole history into memory.
pub struct SessionJournalReader {
    db: sea_orm::DatabaseConnection,
}

impl SessionJournalReader {
    /// Ordered bounded keyset page of Thread commits after `after_sequence`.
    ///
    /// # Errors
    /// Rejects corrupt envelopes or a commit whose key/ownership does not match.
    pub async fn read_page(
        &self,
        thread_id: &str,
        after_sequence: u64,
        limit: std::num::NonZeroUsize,
    ) -> Result<Vec<std::sync::Arc<crate::thread::journal::ThreadCommit>>, super::SessionStoreError>
    {
        read_commit_page(&self.db, thread_id, after_sequence, limit).await
    }

    /// Closes the read-only connection.
    ///
    /// # Errors
    /// Returns a database error if closing fails.
    pub async fn close(self) -> Result<(), super::SessionStoreError> {
        self.db.close().await.map_err(Into::into)
    }
}

/// Opens a read-only, non-hydrating journal reader over an existing session database.
///
/// # Errors
/// Rejects a missing, empty, non-regular or unsupported-version database; existing data
/// is preserved and nothing is written.
pub async fn open_journal_reader(
    options: super::SqliteSessionOptions,
) -> Result<SessionJournalReader, super::SessionStoreError> {
    Ok(SessionJournalReader {
        db: super::sqlite::open_read_only(&options.path).await?,
    })
}

async fn read_commit_page(
    db: &impl sea_orm::ConnectionTrait,
    thread_id: &str,
    after_sequence: u64,
    limit: std::num::NonZeroUsize,
) -> Result<Vec<std::sync::Arc<crate::thread::journal::ThreadCommit>>, super::SessionStoreError> {
    let lower_bound = format!("{COMMIT_RESOURCE_PREFIX}{after_sequence:020}");
    let prefix_len = i64::try_from(COMMIT_RESOURCE_PREFIX.len())
        .map_err(|_| super::SessionStoreError::Invalid("commit id prefix overflow".into()))?;
    let limit = i64::try_from(limit.get())
        .map_err(|_| super::SessionStoreError::Invalid("commit page limit overflow".into()))?;
    let rows = db
        .query_all_raw(super::sqlite::statement(
            "SELECT * FROM session_entries WHERE session_id=? AND id>? \
             AND substr(id,1,?)=? ORDER BY id LIMIT ?",
            vec![
                thread_id.into(),
                lower_bound.into(),
                prefix_len.into(),
                COMMIT_RESOURCE_PREFIX.into(),
                limit.into(),
            ],
        ))
        .await?;
    rows.into_iter()
        .map(|row| decode_thread_commit(thread_id, super::sqlite::decode_row(row)?))
        .collect()
}

/// Decodes one reserved commit resource, enforcing Thread ownership and sequence keying.
fn decode_thread_commit(
    thread_id: &str,
    entry: crate::storage::SessionEntry,
) -> Result<std::sync::Arc<crate::thread::journal::ThreadCommit>, super::SessionStoreError> {
    let payload =
        crate::context::OpaquePayload::new(entry.type_id, entry.schema_version, entry.payload)
            .map_err(|error| super::SessionStoreError::Invalid(error.to_string()))?;
    let commit = crate::thread::journal::ThreadCommit::decode(&payload)
        .map_err(|error| super::SessionStoreError::Invalid(error.to_string()))?;
    if commit.thread_id != thread_id
        || entry.id != format!("{COMMIT_RESOURCE_PREFIX}{:020}", commit.sequence)
    {
        return Err(super::SessionStoreError::Invalid(
            "Thread commit ownership or sequence key mismatch".into(),
        ));
    }
    Ok(std::sync::Arc::new(commit))
}

//! Append-only entry changes. Replay applies envelopes without loading payload codecs.
use crate::storage::{SessionEntryChange, SessionEntryCommit, replay_entries, validate_commit};

use sea_orm::{ConnectionTrait, TransactionTrait};

use crate::storage::SessionEntry;

use super::{SessionStoreError, SqliteSessionStore, sqlite};

impl SqliteSessionStore {
    /// Reads committed history through an optional inclusive sequence watermark.
    ///
    /// # Errors
    /// Rejects broken ordering, mismatched ownership, or corrupt content. No tool, model,
    /// renderer, or business decoder is called.
    pub async fn read_entry_history(
        &self,
        session_id: &str,
        through: Option<u64>,
    ) -> Result<Vec<SessionEntryCommit>, SessionStoreError> {
        let tx = self.owner.shared.db.begin().await?;
        let history = read(&tx, session_id, through).await?;
        tx.commit().await?;
        Ok(history)
    }

    /// Replays raw entries at a committed history watermark without activating a session.
    ///
    /// # Errors
    /// Returns storage or history-integrity errors, including deletion of a missing entry.
    pub async fn replay_entries(
        &self,
        session_id: &str,
        through: Option<u64>,
    ) -> Result<Vec<SessionEntry>, SessionStoreError> {
        let history = self.read_entry_history(session_id, through).await?;
        Ok(replay_entries(&history)?)
    }
}

pub(super) async fn append(
    db: &impl ConnectionTrait,
    session_id: &str,
    thread_revision: Option<u64>,
    changes: Vec<SessionEntryChange>,
) -> Result<(), SessionStoreError> {
    let row = db.query_one_raw(sqlite::statement(
        "SELECT COALESCE(MAX(sequence),0)+1 AS next FROM session_entry_history WHERE session_id=?",
        vec![session_id.into()],
    )).await?.ok_or_else(|| SessionStoreError::Invalid("missing history sequence".into()))?;
    let sequence = row.try_get::<i64>("", "next")?;
    let head = db
        .query_one_raw(sqlite::statement(
            "SELECT sequence FROM session_history_heads WHERE session_id=?",
            vec![session_id.into()],
        ))
        .await?;
    let previous = head
        .map(|row| row.try_get::<i64>("", "sequence"))
        .transpose()?
        .unwrap_or(0);
    if previous.checked_add(1) != Some(sequence) {
        return Err(SessionStoreError::Invalid(
            "history tail differs from its committed head".into(),
        ));
    }
    let batch = SessionEntryCommit {
        session_id: session_id.to_owned(),
        sequence: u64::try_from(sequence)
            .map_err(|_| SessionStoreError::Invalid("history sequence overflow".into()))?,
        thread_revision,
        changes,
    };
    validate_commit(&batch, session_id, batch.sequence)?;
    let envelope = serde_json::to_string(&batch)?;
    let digest = crate::context::content_hash(envelope.as_bytes());
    db.execute_raw(sqlite::statement(
        "INSERT INTO session_entry_history(session_id,sequence,envelope,payload_hash) VALUES(?,?,?,?)",
        vec![session_id.into(), sequence.into(), envelope.into(), digest.clone().into()],
    )).await?;
    db.execute_raw(sqlite::statement(
        "INSERT INTO session_history_heads(session_id,sequence,payload_hash) VALUES(?,?,?) ON CONFLICT(session_id) DO UPDATE SET sequence=excluded.sequence,payload_hash=excluded.payload_hash",
        vec![session_id.into(), sequence.into(), digest.into()],
    )).await?;
    Ok(())
}

async fn read(
    db: &impl ConnectionTrait,
    session_id: &str,
    through: Option<u64>,
) -> Result<Vec<SessionEntryCommit>, SessionStoreError> {
    let limit = through
        .map(i64::try_from)
        .transpose()
        .map_err(|_| SessionStoreError::Invalid("history watermark exceeds SQLite range".into()))?;
    let head = db
        .query_one_raw(sqlite::statement(
            "SELECT sequence,payload_hash FROM session_history_heads WHERE session_id=?",
            vec![session_id.into()],
        ))
        .await?;
    let head = head
        .map(|row| {
            Ok::<_, SessionStoreError>((
                row.try_get::<i64>("", "sequence")?,
                row.try_get::<String>("", "payload_hash")?,
            ))
        })
        .transpose()?;
    let head_sequence = u64::try_from(head.as_ref().map_or(0, |head| head.0))
        .map_err(|_| SessionStoreError::Invalid("negative history head".into()))?;
    let expected = through.unwrap_or(head_sequence);
    if expected > head_sequence {
        return Err(SessionStoreError::Invalid(
            "history watermark is not committed".into(),
        ));
    }
    let mut last_digest = None;
    let rows = db.query_all_raw(sqlite::statement(
        "SELECT sequence,envelope,payload_hash FROM session_entry_history WHERE session_id=? AND (? IS NULL OR sequence<=?) ORDER BY sequence",
        vec![session_id.into(), limit.into(), limit.into()],
    )).await?;
    let mut history = Vec::with_capacity(rows.len());
    for row in rows {
        let envelope: String = row.try_get("", "envelope")?;
        let digest: String = row.try_get("", "payload_hash")?;
        if crate::context::content_hash(envelope.as_bytes()) != digest {
            return Err(SessionStoreError::Invalid(
                "history content digest mismatch".into(),
            ));
        }
        let commit: SessionEntryCommit = serde_json::from_str(&envelope)?;
        let sequence: i64 = row.try_get("", "sequence")?;
        if u64::try_from(sequence).ok() != Some(commit.sequence) {
            return Err(SessionStoreError::Invalid(
                "history sequence/index mismatch".into(),
            ));
        }
        validate_commit(&commit, session_id, history.len() as u64 + 1)?;
        last_digest = Some(digest);
        history.push(commit);
    }
    if history.len() as u64 != expected {
        return Err(SessionStoreError::Invalid(
            "history watermark is not committed".into(),
        ));
    }
    if expected == head_sequence
        && last_digest.as_deref() != head.as_ref().map(|head| head.1.as_str())
    {
        return Err(SessionStoreError::Invalid(
            "history head digest mismatch".into(),
        ));
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sea_orm::TransactionTrait;

    use super::*;

    fn entry(content: &str, revision: u64) -> SessionEntry {
        SessionEntry {
            session_id: "thread".into(),
            id: "foreign".into(),
            type_id: "unknown/future".into(),
            schema_version: 99,
            ordinal: 0,
            turn_id: Some("turn".into()),
            revision,
            created_at: 1,
            updated_at: revision as i64,
            payload: content.into(),
        }
    }

    #[tokio::test]
    async fn history_replays_unknown_text_before_replacement_and_deletion() {
        let store = SqliteSessionStore::open_memory().await.unwrap();
        let original = entry(" {\"n\":90071992547409931234567890} \r\n\0🦀", 1);
        let replacement = entry("not JSON\n", 2);
        for (revision, change) in [
            (
                1,
                SessionEntryChange::Put {
                    entry: original.clone(),
                },
            ),
            (
                2,
                SessionEntryChange::Put {
                    entry: replacement.clone(),
                },
            ),
            (
                3,
                SessionEntryChange::Delete {
                    entry_id: "foreign".into(),
                },
            ),
        ] {
            let tx = store.owner.shared.db.begin().await.unwrap();
            append(&tx, "thread", Some(revision), vec![change])
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
        assert_eq!(
            store.replay_entries("thread", Some(1)).await.unwrap(),
            vec![original]
        );
        assert_eq!(
            store.replay_entries("thread", Some(2)).await.unwrap(),
            vec![replacement]
        );
        assert_eq!(
            store.replay_entries("thread", None).await.unwrap(),
            Vec::<SessionEntry>::new()
        );
        assert!(store.replay_entries("thread", Some(4)).await.is_err());
        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn missing_tail_cannot_be_replayed_or_reused_for_a_new_commit() {
        let store = SqliteSessionStore::open_memory().await.unwrap();
        let change = SessionEntryChange::Put {
            entry: entry("raw", 1),
        };
        let tx = store.owner.shared.db.begin().await.unwrap();
        append(&tx, "thread", Some(1), vec![change.clone()])
            .await
            .unwrap();
        tx.commit().await.unwrap();
        store
            .owner
            .shared
            .db
            .execute_unprepared("DELETE FROM session_entry_history")
            .await
            .unwrap();
        assert!(store.replay_entries("thread", None).await.is_err());
        let tx = store.owner.shared.db.begin().await.unwrap();
        assert!(append(&tx, "thread", Some(2), vec![change]).await.is_err());
        tx.rollback().await.unwrap();
        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn rolled_back_changes_never_enter_history_and_corruption_is_reported() {
        let store = SqliteSessionStore::open_memory().await.unwrap();
        let change = SessionEntryChange::Put {
            entry: entry("raw", 1),
        };
        let tx = store.owner.shared.db.begin().await.unwrap();
        append(&tx, "thread", Some(1), vec![change.clone()])
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        assert!(
            store
                .read_entry_history("thread", None)
                .await
                .unwrap()
                .is_empty()
        );
        let tx = store.owner.shared.db.begin().await.unwrap();
        append(&tx, "thread", Some(1), vec![change]).await.unwrap();
        tx.commit().await.unwrap();
        store
            .owner
            .shared
            .db
            .execute_unprepared("UPDATE session_entry_history SET payload_hash='corrupt'")
            .await
            .unwrap();
        assert!(store.replay_entries("thread", None).await.is_err());
        store.shutdown().await.unwrap();
    }
}

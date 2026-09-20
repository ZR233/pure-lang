use std::path::PathBuf;
use std::time::Duration;

use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
    TransactionTrait,
};

use crate::storage::SessionEntry;
/// SQLite session database configuration; no product database or ORM handle is required.
#[derive(Debug, Clone)]
pub struct SqliteSessionOptions {
    pub path: PathBuf,
}

/// Cold storage and writer failures.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    #[error("invalid cold history: {0}")]
    Replay(#[from] crate::storage::ReplayError),
    #[error("unsupported session schema {found}; expected {supported}; existing data preserved")]
    UnsupportedSchema { found: i64, supported: i64 },
    #[error("session database operation failed: {0}")]
    Database(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error(
        "session initialization failed ({initialization}); connection cleanup failed ({cleanup})"
    )]
    InitializationCleanup {
        #[source]
        initialization: Box<SessionStoreError>,
        cleanup: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("session payload codec failed: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("session filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("session {session_id} revision conflict: expected {expected:?}, actual {actual:?}")]
    Conflict {
        session_id: String,
        expected: Option<u64>,
        actual: Option<u64>,
    },
    #[error("invalid session storage data: {0}")]
    Invalid(String),
    #[error("session writer stopped before the requested revision was durable")]
    Stopped,
    #[error("session writer panicked; pending facts retained")]
    Panicked,
}

impl From<sea_orm::DbErr> for SessionStoreError {
    fn from(source: sea_orm::DbErr) -> Self {
        Self::Database(Box::new(source))
    }
}

impl SessionStoreError {
    pub(super) fn retryable(&self) -> bool {
        match self {
            Self::Database(error) => {
                let message = error.to_string().to_ascii_lowercase();
                message.contains("locked")
                    || message.contains("busy")
                    || message.contains("temporarily")
            }
            Self::InitializationCleanup { .. }
            | Self::Replay(_)
            | Self::Codec(_)
            | Self::UnsupportedSchema { .. }
            | Self::Io(_)
            | Self::Conflict { .. }
            | Self::Invalid(_)
            | Self::Stopped
            | Self::Panicked => false,
        }
    }
}

pub(super) async fn open(
    options: Option<SqliteSessionOptions>,
    require_existing: bool,
) -> Result<DatabaseConnection, SessionStoreError> {
    // `fresh` is true only when the backing file did not exist before this open, so an
    // existing empty/garbage file is never silently initialized as a new database.
    let fresh = if let Some(options) = options {
        if let Some(parent) = options.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if !options.path.is_absolute() {
            return Err(SessionStoreError::Invalid(
                "session database path must be absolute".into(),
            ));
        }
        let existed = tokio::fs::try_exists(&options.path).await?;
        if require_existing && !existed {
            return Err(SessionStoreError::Invalid(format!(
                "session database does not exist: {}",
                options.path.display()
            )));
        }
        let mut url = url::Url::parse("sqlite:///")
            .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
        url.set_path(options.path.to_str().ok_or_else(|| {
            SessionStoreError::Invalid("session database path is not UTF-8".into())
        })?);
        url.set_query(Some(if existed { "mode=rw" } else { "mode=rwc" }));
        (!existed, url.to_string())
    } else {
        (true, "sqlite::memory:".to_owned())
    };
    let (fresh, url) = fresh;
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(1)
        .connect_timeout(Duration::from_secs(10))
        // SQLite applies these per connection, so they must be set as connect options rather
        // than once with a PRAGMA statement (which would only touch a single pooled connection).
        .map_sqlx_sqlite_opts(|options| {
            options
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full)
                .busy_timeout(Duration::from_secs(5))
                .foreign_keys(true)
        });
    let db = Database::connect(options).await?;
    if let Err(initialization) = initialize(&db, fresh).await {
        return match db.close().await {
            Ok(()) => Err(initialization),
            Err(cleanup) => Err(SessionStoreError::InitializationCleanup {
                initialization: Box::new(initialization),
                cleanup: Box::new(cleanup),
            }),
        };
    }
    Ok(db)
}

/// Opens an existing session database read-only, without hydrating or writing anything.
///
/// It starts no writer and does not run `initialize` (no WAL/table/version writes); a
/// missing, empty, non-regular or unsupported-version file is a typed error and is preserved.
pub(super) async fn open_read_only(
    path: &std::path::Path,
) -> Result<DatabaseConnection, SessionStoreError> {
    if !path.is_absolute() {
        return Err(SessionStoreError::Invalid(
            "session database path must be absolute".into(),
        ));
    }
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.is_file() {
        return Err(SessionStoreError::Invalid(
            "session database is not a regular file".into(),
        ));
    }
    if metadata.len() == 0 {
        return Err(SessionStoreError::UnsupportedSchema {
            found: 0,
            supported: super::SESSION_SCHEMA_VERSION,
        });
    }
    let mut url = url::Url::parse("sqlite:///")
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
    url.set_path(
        path.to_str().ok_or_else(|| {
            SessionStoreError::Invalid("session database path is not UTF-8".into())
        })?,
    );
    url.set_query(Some("mode=ro"));
    let mut options = ConnectOptions::new(url.to_string());
    options
        .max_connections(1)
        .connect_timeout(Duration::from_secs(10))
        // A read-only connection must not attempt to change the journal mode; it still gets
        // a busy timeout so concurrent readers wait instead of failing immediately.
        .map_sqlx_sqlite_opts(|options| {
            options
                .busy_timeout(Duration::from_secs(5))
                .foreign_keys(true)
        });
    let db = Database::connect(options).await?;
    let version = db
        .query_one_raw(statement("PRAGMA user_version", vec![]))
        .await?
        .ok_or_else(|| SessionStoreError::Invalid("missing SQLite schema version".into()))?
        .try_get::<i64>("", "user_version")?;
    if version != super::SESSION_SCHEMA_VERSION {
        let _ = db.close().await;
        return Err(SessionStoreError::UnsupportedSchema {
            found: version,
            supported: super::SESSION_SCHEMA_VERSION,
        });
    }
    Ok(db)
}

async fn initialize(db: &DatabaseConnection, fresh: bool) -> Result<(), SessionStoreError> {
    let version = db
        .query_one_raw(statement("PRAGMA user_version", vec![]))
        .await?
        .ok_or_else(|| SessionStoreError::Invalid("missing SQLite schema version".into()))?
        .try_get::<i64>("", "user_version")?;
    if version != 0 && version != super::SESSION_SCHEMA_VERSION {
        return Err(SessionStoreError::UnsupportedSchema {
            found: version,
            supported: super::SESSION_SCHEMA_VERSION,
        });
    }
    if version == 0 {
        // Only a database file that did not exist may be initialized; an existing
        // unversioned/empty file is preserved and rejected instead of being overwritten.
        if !fresh {
            return Err(SessionStoreError::UnsupportedSchema {
                found: 0,
                supported: super::SESSION_SCHEMA_VERSION,
            });
        }
    }
    // WAL, synchronous, busy timeout and foreign keys are configured per connection through
    // the connect options above; no PRAGMA is issued here.
    if version == super::SESSION_SCHEMA_VERSION {
        return Ok(());
    }
    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS session_entries (
            session_id TEXT NOT NULL, id TEXT NOT NULL, type_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL, turn_id TEXT, envelope TEXT NOT NULL CHECK(json_valid(envelope)), payload_hash TEXT NOT NULL,
            PRIMARY KEY(session_id,id));
         CREATE INDEX IF NOT EXISTS session_entries_kind ON session_entries(session_id,type_id,ordinal,id);
         CREATE INDEX IF NOT EXISTS session_entries_turn ON session_entries(session_id,turn_id,type_id,ordinal);
         CREATE TABLE IF NOT EXISTS session_entry_history (
            session_id TEXT NOT NULL, sequence INTEGER NOT NULL,
            envelope TEXT NOT NULL, payload_hash TEXT NOT NULL,
            PRIMARY KEY(session_id,sequence));
         CREATE TABLE IF NOT EXISTS session_history_heads (
            session_id TEXT PRIMARY KEY, sequence INTEGER NOT NULL, payload_hash TEXT NOT NULL);"
    ).await?;
    db.execute_unprepared(&format!(
        "PRAGMA user_version={}",
        super::SESSION_SCHEMA_VERSION
    ))
    .await?;
    Ok(())
}

pub(super) fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

pub(super) async fn entries(
    db: &impl ConnectionTrait,
    session_id: &str,
    type_id: Option<&str>,
) -> Result<Vec<SessionEntry>, SessionStoreError> {
    let rows = match type_id {
        Some(kind) => db.query_all_raw(statement("SELECT * FROM session_entries WHERE session_id=? AND type_id=? ORDER BY ordinal,id", vec![session_id.into(),kind.into()])).await?,
        None => db.query_all_raw(statement("SELECT * FROM session_entries WHERE session_id=? ORDER BY ordinal,id", vec![session_id.into()])).await?,
    };
    rows.into_iter().map(decode_row).collect()
}

pub(super) fn decode_row(row: sea_orm::QueryResult) -> Result<SessionEntry, SessionStoreError> {
    let encoded: String = row.try_get("", "envelope")?;
    let hash: String = row.try_get("", "payload_hash")?;
    if crate::context::content_hash(encoded.as_bytes()) != hash {
        return Err(SessionStoreError::Invalid(
            "session entry integrity check failed".into(),
        ));
    }
    let entry: SessionEntry = serde_json::from_str(&encoded)?;
    if entry.session_id != row.try_get::<String>("", "session_id")?
        || entry.id != row.try_get::<String>("", "id")?
        || entry.type_id != row.try_get::<String>("", "type_id")?
        || integer(entry.ordinal)? != row.try_get::<i64>("", "ordinal")?
        || entry.turn_id != row.try_get::<Option<String>>("", "turn_id")?
        || entry.schema_version == 0
    {
        return Err(SessionStoreError::Invalid(
            "session entry envelope/index mismatch".into(),
        ));
    }
    Ok(entry)
}

pub(super) async fn put(
    db: &impl ConnectionTrait,
    entry: &SessionEntry,
) -> Result<Option<SessionEntry>, SessionStoreError> {
    let mut entry = entry.clone();
    if let Some(previous) = db
        .query_one_raw(statement(
            "SELECT * FROM session_entries WHERE session_id=? AND id=?",
            vec![entry.session_id.clone().into(), entry.id.clone().into()],
        ))
        .await?
    {
        let previous = decode_row(previous)?;
        entry.created_at = previous.created_at;
        if entry == previous {
            return Ok(None);
        }
    }
    let encoded = serde_json::to_string(&entry)?;
    let hash = crate::context::content_hash(encoded.as_bytes());
    db.execute_raw(statement("INSERT INTO session_entries(session_id,id,type_id,ordinal,turn_id,envelope,payload_hash) VALUES(?,?,?,?,?,?,?) ON CONFLICT(session_id,id) DO UPDATE SET type_id=excluded.type_id,ordinal=excluded.ordinal,turn_id=excluded.turn_id,envelope=excluded.envelope,payload_hash=excluded.payload_hash",
        vec![entry.session_id.clone().into(),entry.id.clone().into(),entry.type_id.clone().into(),integer(entry.ordinal)?.into(),entry.turn_id.clone().into(),encoded.into(),hash.into()])).await?;
    Ok(Some(entry))
}

fn integer(value: u64) -> Result<i64, SessionStoreError> {
    i64::try_from(value).map_err(|_| SessionStoreError::Invalid("SQLite integer overflow".into()))
}

pub(super) async fn apply(
    db: &DatabaseConnection,
    entries: &[std::sync::Arc<SessionEntry>],
) -> Result<(), SessionStoreError> {
    let tx = db.begin().await?;
    for entry in entries {
        let previous = tx
            .query_one_raw(statement(
                "SELECT * FROM session_entries WHERE session_id=? AND id=?",
                vec![entry.session_id.clone().into(), entry.id.clone().into()],
            ))
            .await?;
        if let Some(previous) = previous {
            let previous = decode_row(previous)?;
            // Immutability is a property of the content; a re-admitted record (for example
            // after reopening a store that no longer caches journal rows) keeps the stored
            // row and only its freshly computed metadata is ignored.
            if previous.type_id != entry.type_id
                || previous.schema_version != entry.schema_version
                || previous.payload != entry.payload
            {
                return Err(SessionStoreError::Invalid(format!(
                    "immutable resource {} changed",
                    entry.id
                )));
            }
        } else if let Some(saved) = put(&tx, entry).await? {
            super::history::append(
                &tx,
                &entry.session_id,
                None,
                vec![super::SessionEntryChange::Put { entry: saved }],
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SessionEntry;
    use std::sync::Arc;

    fn record(payload: &str, created_at: i64) -> SessionEntry {
        SessionEntry {
            session_id: "thread".into(),
            id: "pl.resource.record".into(),
            type_id: "pl.core.record".into(),
            schema_version: 1,
            ordinal: 1,
            revision: 1,
            turn_id: None,
            created_at,
            updated_at: created_at,
            payload: payload.into(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_second_connection_waits_on_the_busy_timeout_instead_of_failing_busy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("concurrent.sqlite");
        let first = open(Some(SqliteSessionOptions { path: path.clone() }), false)
            .await
            .unwrap();
        let second = open(Some(SqliteSessionOptions { path: path.clone() }), true)
            .await
            .unwrap();

        // The busy timeout is a per-connection option and must be present on this connection.
        let timeout = second
            .query_one_raw(statement("PRAGMA busy_timeout", vec![]))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "timeout")
            .unwrap();
        assert_eq!(timeout, 5000);

        // `first` holds a write transaction; `second` must wait for it rather than fail.
        let tx = first.begin().await.unwrap();
        tx.execute_unprepared(
            "INSERT INTO session_entries(session_id,id,type_id,ordinal,turn_id,envelope,payload_hash) \
             VALUES('thread','pl.resource.a','t',1,NULL,'{}','h')",
        )
        .await
        .unwrap();
        let writer = tokio::spawn(async move {
            let tx = second.begin().await.unwrap();
            tx.execute_unprepared(
                "INSERT INTO session_entries(session_id,id,type_id,ordinal,turn_id,envelope,payload_hash) \
                 VALUES('thread','pl.resource.b','t',2,NULL,'{}','h')",
            )
            .await
        });
        for _ in 0..32 {
            tokio::task::yield_now().await;
        }
        tx.commit().await.unwrap();
        let result = writer.await.unwrap();
        assert!(
            result.is_ok(),
            "second writer must wait, not fail BUSY: {result:?}"
        );

        // Without the connect options a fresh connection has busy_timeout = 0 and fails
        // immediately while the lock is held (the failure mode the options prevent).
        let plain = Database::connect(format!("sqlite://{}?mode=rw", path.display()))
            .await
            .unwrap();
        let held = first.begin().await.unwrap();
        held.execute_unprepared(
            "INSERT INTO session_entries(session_id,id,type_id,ordinal,turn_id,envelope,payload_hash) \
             VALUES('thread','pl.resource.c','t',3,NULL,'{}','h')",
        )
        .await
        .unwrap();
        let immediate = plain
            .execute_unprepared(
                "INSERT INTO session_entries(session_id,id,type_id,ordinal,turn_id,envelope,payload_hash) \
                 VALUES('thread','pl.resource.d','t',4,NULL,'{}','h')",
            )
            .await;
        assert!(
            immediate.is_err(),
            "a connection without a busy timeout fails immediately"
        );
        held.rollback().await.unwrap();
        first.close().await.unwrap();
        plain.close().await.unwrap();
    }

    #[tokio::test]
    async fn readmission_keeps_the_stored_row_but_changed_content_is_rejected() {
        let db = open(None, false).await.unwrap();
        apply(&db, &[Arc::new(record("A", 1))]).await.unwrap();
        // Same content with fresh metadata (fresh created_at) is idempotent: the stored row
        // is kept and the new metadata is ignored.
        apply(&db, &[Arc::new(record("A", 99))]).await.unwrap();
        let rows = entries(&db, "thread", None).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].created_at, 1);
        assert_eq!(rows[0].payload, "A");
        // The same id with different content must still be rejected.
        let error = apply(&db, &[Arc::new(record("B", 100))]).await.unwrap_err();
        assert!(matches!(error, SessionStoreError::Invalid(_)), "{error}");
        assert_eq!(entries(&db, "thread", None).await.unwrap()[0].payload, "A");
        db.close().await.unwrap();
    }
}

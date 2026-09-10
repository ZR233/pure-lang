use std::path::PathBuf;
use std::time::Duration;

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
) -> Result<DatabaseConnection, SessionStoreError> {
    let url = if let Some(options) = options {
        if let Some(parent) = options.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if !options.path.is_absolute() {
            return Err(SessionStoreError::Invalid(
                "session database path must be absolute".into(),
            ));
        }
        let mut url = url::Url::parse("sqlite:///")
            .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
        url.set_path(options.path.to_str().ok_or_else(|| {
            SessionStoreError::Invalid("session database path is not UTF-8".into())
        })?);
        url.set_query(Some("mode=rwc"));
        url.to_string()
    } else {
        "sqlite::memory:".to_owned()
    };
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(1)
        .connect_timeout(Duration::from_secs(10));
    let db = Database::connect(options).await?;
    if let Err(initialization) = initialize(&db).await {
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

async fn initialize(db: &DatabaseConnection) -> Result<(), SessionStoreError> {
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
        let existing = db.query_one_raw(statement(
            "SELECT name FROM sqlite_schema WHERE type='table' AND substr(name,1,7)<>'sqlite_' LIMIT 1",
            vec![],
        )).await?;
        if existing.is_some() {
            return Err(SessionStoreError::UnsupportedSchema {
                found: 0,
                supported: super::SESSION_SCHEMA_VERSION,
            });
        }
    }
    db.execute_unprepared(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=5000;",
    )
    .await?;
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
            if decode_row(previous)? != **entry {
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

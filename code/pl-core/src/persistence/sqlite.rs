use std::path::PathBuf;
use std::time::Duration;

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
    TransactionTrait,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::session::entry::SessionEntry;
use crate::{ThreadActorState, ThreadCommit};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextManifest {
    transcript_len: usize,
}

/// SQLite session database configuration; no product database or ORM handle is required.
#[derive(Debug, Clone)]
pub struct SqliteSessionOptions {
    pub path: PathBuf,
}

/// Cold storage and writer failures.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    #[error("session database operation failed: {0}")]
    Database(#[from] sea_orm::DbErr),
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

impl SessionStoreError {
    pub(super) fn retryable(&self) -> bool {
        match self {
            Self::Database(error) => {
                let message = error.to_string().to_ascii_lowercase();
                message.contains("locked")
                    || message.contains("busy")
                    || message.contains("temporarily")
            }
            Self::Codec(_)
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
    let version = db
        .query_one_raw(statement("PRAGMA user_version", vec![]))
        .await?
        .ok_or_else(|| SessionStoreError::Invalid("missing SQLite schema version".into()))?
        .try_get::<i64>("", "user_version")?;
    if !(0..=4).contains(&version) {
        return Err(SessionStoreError::Invalid(format!(
            "unsupported session schema {version}"
        )));
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
         CREATE TABLE IF NOT EXISTS session_receipts (
            session_id TEXT NOT NULL, revision INTEGER NOT NULL, payload_hash TEXT NOT NULL,
            PRIMARY KEY(session_id,revision));"
    ).await?;
    if version < 4 {
        super::task_records::migrate(&db, version).await?;
    }
    Ok(db)
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

pub(super) async fn read<T: DeserializeOwned>(
    db: &impl ConnectionTrait,
    session_id: &str,
    id: &str,
) -> Result<Option<T>, SessionStoreError> {
    let row = db
        .query_one_raw(statement(
            "SELECT * FROM session_entries WHERE session_id=? AND id=?",
            vec![session_id.into(), id.into()],
        ))
        .await?;
    row.map(|row| {
        let entry = decode_row(row)?;
        if entry.schema_version != 1 {
            return Err(SessionStoreError::Invalid(format!(
                "unsupported {} version {}",
                entry.type_id, entry.schema_version
            )));
        }
        Ok(serde_json::from_value(entry.payload)?)
    })
    .transpose()
}

pub(super) fn decode_row(row: sea_orm::QueryResult) -> Result<SessionEntry, SessionStoreError> {
    let encoded: String = row.try_get("", "envelope")?;
    let hash: String = row.try_get("", "payload_hash")?;
    if crate::canonical_content_hash(encoded.as_bytes()) != hash {
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

pub(super) fn decode_builtin<T: DeserializeOwned>(
    entry: SessionEntry,
) -> Result<T, SessionStoreError> {
    if entry.schema_version != 1 {
        return Err(SessionStoreError::Invalid(format!(
            "unsupported {} version {}",
            entry.type_id, entry.schema_version
        )));
    }
    Ok(serde_json::from_value(entry.payload)?)
}

pub(super) async fn put(
    db: &impl ConnectionTrait,
    entry: &SessionEntry,
) -> Result<(), SessionStoreError> {
    let mut entry = entry.clone();
    if let Some(previous) = db
        .query_one_raw(statement(
            "SELECT * FROM session_entries WHERE session_id=? AND id=?",
            vec![entry.session_id.clone().into(), entry.id.clone().into()],
        ))
        .await?
    {
        let previous = decode_row(previous)?;
        if entry.type_id == "pl.toolResult" || previous.type_id == "pl.toolResult" {
            if entry.type_id != previous.type_id
                || entry.schema_version != previous.schema_version
                || entry.turn_id != previous.turn_id
                || entry.payload != previous.payload
            {
                return Err(SessionStoreError::Invalid(format!(
                    "immutable task result {} changed",
                    entry.id
                )));
            }
            return Ok(());
        }
        entry.created_at = previous.created_at;
    }
    let encoded = serde_json::to_string(&entry)?;
    let hash = crate::canonical_content_hash(encoded.as_bytes());
    db.execute_raw(statement("INSERT INTO session_entries(session_id,id,type_id,ordinal,turn_id,envelope,payload_hash) VALUES(?,?,?,?,?,?,?) ON CONFLICT(session_id,id) DO UPDATE SET type_id=excluded.type_id,ordinal=excluded.ordinal,turn_id=excluded.turn_id,envelope=excluded.envelope,payload_hash=excluded.payload_hash",
        vec![entry.session_id.into(),entry.id.into(),entry.type_id.into(),integer(entry.ordinal)?.into(),entry.turn_id.into(),encoded.into(),hash.into()])).await?;
    Ok(())
}

fn integer(value: u64) -> Result<i64, SessionStoreError> {
    i64::try_from(value).map_err(|_| SessionStoreError::Invalid("SQLite integer overflow".into()))
}

fn builtin<T: Serialize>(
    commit: &ThreadCommit,
    id: String,
    kind: &str,
    ordinal: u64,
    turn_id: Option<String>,
    payload: &T,
) -> Result<SessionEntry, SessionStoreError> {
    Ok(SessionEntry {
        session_id: commit.agent_id.to_string(),
        id,
        type_id: kind.to_owned(),
        schema_version: 1,
        ordinal,
        turn_id,
        revision: commit.next_state.snapshot.revision,
        created_at: commit.next_state.snapshot.updated_at,
        updated_at: commit.next_state.snapshot.updated_at,
        payload: serde_json::to_value(payload)?,
    })
}

pub(super) async fn apply(
    db: &DatabaseConnection,
    commits: &[super::writer::PendingOperation],
) -> Result<(), SessionStoreError> {
    let tx = db.begin().await?;
    for operation in commits {
        let commit = match operation {
            super::writer::PendingOperation::Thread(commit) => commit,
            super::writer::PendingOperation::Resource(entry) => {
                let previous = tx
                    .query_one_raw(statement(
                        "SELECT * FROM session_entries WHERE session_id=? AND id=?",
                        vec![entry.session_id.clone().into(), entry.id.clone().into()],
                    ))
                    .await?;
                if let Some(previous) = previous {
                    let previous = decode_row(previous)?;
                    if previous.type_id != entry.type_id
                        || previous.schema_version != entry.schema_version
                        || previous.payload != entry.payload
                    {
                        return Err(SessionStoreError::Invalid(format!(
                            "immutable resource {} changed",
                            entry.id
                        )));
                    }
                } else {
                    put(&tx, entry).await?;
                }
                continue;
            }
        };
        let mut state = commit.next_state.clone();
        let task_records = super::task_records::encode(&state)?;
        state.session.tasks = crate::session_runtime::SessionTasks::default();
        let extensions = state.session.session.working_state().entries.clone();
        let mut working_state = state.session.session.working_state().clone();
        working_state.entries.clear();
        state.session.session = crate::AgentSession::from_snapshot(crate::AgentSessionSnapshot {
            transcript: Vec::new(),
            working_state,
        });
        let mut records = vec![builtin(
            commit,
            "pl.actor".into(),
            "pl.actor",
            0,
            None,
            &state,
        )?];
        records.extend(task_records);
        records.push(builtin(
            commit,
            "pl.contextManifest".into(),
            "pl.contextManifest",
            0,
            None,
            &ContextManifest {
                transcript_len: commit.next_state.session.session.items().len(),
            },
        )?);
        let replace_context = matches!(
            commit.facts.context,
            Some(crate::ThreadContextMutation::Replace { .. })
        );
        if let Some(context) = &commit.facts.context {
            let (start, items) = match context {
                crate::ThreadContextMutation::Append { items } => (
                    commit
                        .next_state
                        .session
                        .session
                        .items()
                        .len()
                        .checked_sub(items.len())
                        .ok_or_else(|| {
                            SessionStoreError::Invalid("transcript suffix exceeds snapshot".into())
                        })?,
                    items,
                ),
                crate::ThreadContextMutation::Replace { items } => (0, items),
            };
            for (offset, item) in items.iter().enumerate() {
                let ordinal = (start + offset) as u64;
                records.push(builtin(
                    commit,
                    format!("pl.context.{ordinal:020}"),
                    "pl.context",
                    ordinal,
                    None,
                    item,
                )?);
            }
        }
        let mut changed_items = std::collections::BTreeMap::new();
        let mut turns = std::collections::BTreeMap::new();
        for event in &commit.facts.notifications {
            match &event.notification {
                crate::ThreadNotification::ItemStarted { item }
                | crate::ThreadNotification::ItemCompleted { item } => {
                    changed_items.insert(item.id.clone(), item.as_ref().clone());
                }
                crate::ThreadNotification::ItemDelta { delta } => {
                    let item = commit
                        .facts
                        .projection_snapshot
                        .as_ref()
                        .and_then(|snapshot| {
                            snapshot.items.iter().find(|item| item.id == delta.item_id)
                        })
                        .ok_or_else(|| {
                            SessionStoreError::Invalid(
                                "item delta has no canonical snapshot".into(),
                            )
                        })?;
                    changed_items.insert(item.id.clone(), item.clone());
                }
                crate::ThreadNotification::TurnStarted { turn }
                | crate::ThreadNotification::TurnUpdated { turn }
                | crate::ThreadNotification::TurnCompleted { turn } => {
                    turns.insert(turn.id.clone(), turn.clone());
                }
                crate::ThreadNotification::InteractionChanged { interaction } => {
                    records.push(builtin(
                        commit,
                        format!("pl.interaction.{}", interaction.interaction_id),
                        "pl.interaction",
                        0,
                        Some(interaction.scope.turn_id.clone()),
                        interaction.as_ref(),
                    )?);
                }
                crate::ThreadNotification::ThreadRuntimeUpdated { .. }
                | crate::ThreadNotification::Lagged { .. } => {}
            }
        }
        if let Some(snapshot) = &commit.facts.projection_snapshot {
            let mut header = snapshot.clone();
            header.items.clear();
            header.interactions.clear();
            records.push(builtin(
                commit,
                "pl.snapshot".into(),
                "pl.snapshot",
                0,
                None,
                &header,
            )?);
            if commit.expected_revision.is_none()
                || matches!(commit.mutation, crate::ThreadMutation::ReplaceThread { .. })
            {
                changed_items.extend(
                    snapshot
                        .items
                        .iter()
                        .map(|item| (item.id.clone(), item.clone())),
                );
                for interaction in &snapshot.interactions {
                    records.push(builtin(
                        commit,
                        format!("pl.interaction.{}", interaction.interaction_id),
                        "pl.interaction",
                        0,
                        Some(interaction.scope.turn_id.clone()),
                        interaction,
                    )?);
                }
            }
        }
        for item in changed_items.into_values() {
            records.push(builtin(
                commit,
                format!("pl.item.{}", item.id),
                "pl.item",
                item.ordinal,
                Some(item.turn_id.clone()),
                &item,
            )?);
        }
        if let Some(turn) = &commit.facts.turn_transition {
            turns.insert(turn.id.clone(), turn.clone());
        }
        let mut next_turn_ordinal = None;
        for turn in turns.into_values() {
            let id = format!("pl.turn.{}", turn.id);
            let row = tx
                .query_one_raw(statement(
                    "SELECT ordinal FROM session_entries WHERE session_id=? AND id=?",
                    vec![commit.agent_id.to_string().into(), id.clone().into()],
                ))
                .await?;
            let ordinal = match row {
                Some(row) => u64::try_from(row.try_get::<i64>("", "ordinal")?)
                    .map_err(|_| SessionStoreError::Invalid("negative Turn ordinal".into()))?,
                None => {
                    let ordinal = if let Some(next) = next_turn_ordinal {
                        next
                    } else {
                        let next = tx.query_one_raw(statement("SELECT COALESCE(MAX(ordinal),-1)+1 AS ordinal FROM session_entries WHERE session_id=? AND type_id='pl.turn'",vec![commit.agent_id.to_string().into()])).await?
                        .ok_or_else(|| SessionStoreError::Invalid("missing Turn ordinal".into()))?.try_get::<i64>("","ordinal")?;
                        u64::try_from(next).map_err(|_| {
                            SessionStoreError::Invalid("Turn ordinal overflow".into())
                        })?
                    };
                    next_turn_ordinal = Some(ordinal.checked_add(1).ok_or_else(|| {
                        SessionStoreError::Invalid("Turn ordinal overflow".into())
                    })?);
                    ordinal
                }
            };
            records.push(builtin(
                commit,
                id,
                "pl.turn",
                ordinal,
                Some(turn.id.clone()),
                &turn,
            )?);
        }
        records.extend(extensions.into_values());
        let payload_hash =
            crate::canonical_json_hash(&serde_json::to_value((replace_context, &records))?);
        let current = tx.query_one_raw(statement("SELECT revision,payload_hash FROM session_receipts WHERE session_id=? ORDER BY revision DESC LIMIT 1",vec![commit.agent_id.to_string().into()])).await?;
        let actual = current
            .as_ref()
            .map(|row| row.try_get::<i64>("", "revision"))
            .transpose()?
            .map(|revision| revision as u64);
        if actual != commit.expected_revision {
            let receipt = tx
                .query_one_raw(statement(
                    "SELECT payload_hash FROM session_receipts WHERE session_id=? AND revision=?",
                    vec![
                        commit.agent_id.to_string().into(),
                        integer(commit.next_state.snapshot.revision)?.into(),
                    ],
                ))
                .await?;
            if receipt
                .as_ref()
                .map(|row| row.try_get::<String>("", "payload_hash"))
                .transpose()?
                .as_deref()
                == Some(&payload_hash)
            {
                continue;
            }
            return Err(SessionStoreError::Conflict {
                session_id: commit.agent_id.to_string(),
                expected: commit.expected_revision,
                actual,
            });
        }
        tx.execute_raw(statement("DELETE FROM session_entries WHERE session_id=? AND substr(type_id,1,3) <> 'pl.' AND substr(id,1,12) <> 'pl.resource.'",vec![commit.agent_id.to_string().into()])).await?;
        tx.execute_raw(statement(
            "DELETE FROM session_entries WHERE session_id=? AND type_id='pl.toolTask'",
            vec![commit.agent_id.to_string().into()],
        ))
        .await?;
        if replace_context {
            tx.execute_raw(statement(
                "DELETE FROM session_entries WHERE session_id=? AND type_id='pl.context'",
                vec![commit.agent_id.to_string().into()],
            ))
            .await?;
        }
        for entry in &records {
            put(&tx, entry).await?;
        }
        tx.execute_raw(statement(
            "INSERT INTO session_receipts(session_id,revision,payload_hash) VALUES(?,?,?)",
            vec![
                commit.agent_id.to_string().into(),
                integer(commit.next_state.snapshot.revision)?.into(),
                payload_hash.into(),
            ],
        ))
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(super) async fn restore(
    db: &impl ConnectionTrait,
    session_id: &str,
) -> Result<Option<crate::RestoredAgentRuntime>, SessionStoreError> {
    let Some(mut state) = read::<ThreadActorState>(db, session_id, "pl.actor").await? else {
        return Ok(None);
    };
    if state.session.tasks.storage_records().next().is_some() {
        return Err(SessionStoreError::Invalid(
            "actor contains noncanonical embedded task records".into(),
        ));
    }
    if state.snapshot.identity.id.as_str() != session_id {
        return Err(SessionStoreError::Invalid(
            "actor belongs to another session".into(),
        ));
    }
    state.session.tasks =
        super::task_records::restore(db, session_id, state.snapshot.revision).await?;
    state
        .session
        .inbox
        .validate_task_references(&state.session.tasks)
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
    // Task/result records have already been validated above; complete bodies are loaded only on demand.
    let records = db.query_all_raw(statement(
        "SELECT * FROM session_entries WHERE session_id=? AND type_id NOT IN ('pl.actor','pl.taskManifest','pl.toolTask','pl.toolResult') ORDER BY ordinal,id",
        vec![session_id.into()],
    )).await?.into_iter().map(decode_row).collect::<Result<Vec<_>, _>>()?;
    let manifest = read::<ContextManifest>(db, session_id, "pl.contextManifest")
        .await?
        .ok_or_else(|| SessionStoreError::Invalid("missing session transcript manifest".into()))?;
    let mut snapshot = read::<crate::ThreadSnapshot>(db, session_id, "pl.snapshot")
        .await?
        .unwrap_or_else(|| crate::ThreadSnapshot::empty(session_id));
    let mut extensions = std::collections::BTreeMap::new();
    let mut transcript = std::collections::BTreeMap::new();
    for entry in records {
        match entry.type_id.as_str() {
            "pl.context" => {
                transcript.insert(
                    entry.ordinal,
                    decode_builtin::<crate::ModelContextItem>(entry)?,
                );
            }
            "pl.item" => snapshot.items.push(decode_builtin(entry)?),
            "pl.interaction" => {
                let interaction: crate::InteractionRequest = decode_builtin(entry)?;
                if interaction.status() == pl_protocol::InteractionStatus::Pending {
                    snapshot.interactions.push(interaction);
                }
            }
            kind if !kind.starts_with("pl.") && !entry.id.starts_with("pl.resource.") => {
                extensions.insert(entry.id.clone(), entry);
            }
            _ => {}
        }
    }
    let mut session = state.session.session.snapshot();
    if transcript.len() != manifest.transcript_len {
        return Err(SessionStoreError::Invalid(
            "session transcript length does not match its checkpoint".into(),
        ));
    }
    session.working_state.entries = extensions;
    for (expected, (ordinal, item)) in transcript.into_iter().enumerate() {
        if ordinal != expected as u64 {
            return Err(SessionStoreError::Invalid(
                "non-contiguous session transcript".into(),
            ));
        }
        session.transcript.push(item);
    }
    snapshot.revision = state.session.thread_revision;
    state.session.session = crate::AgentSession::from_snapshot(session);
    Ok(Some(crate::RestoredAgentRuntime {
        state,
        thread_snapshot: Some(crate::RestoredThreadSnapshot { snapshot }),
    }))
}

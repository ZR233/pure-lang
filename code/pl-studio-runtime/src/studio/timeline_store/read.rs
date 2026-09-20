//! Reader lifecycle and connection for the Studio timeline index.
//!
//! [`TimelineReader`] owns one bounded, **read-only** connection to a per-Thread session database.
//! It never creates or rebuilds the file, never loads the core journal (`session_entries`), and
//! never activates a Thread owner. Missing databases, missing index tables and unknown schema
//! versions are explicit typed errors, never an empty page.
//!
//! The page, keyed-fact and content concerns live in [`super::page`], [`super::facts`] and
//! [`super::content`]; this module holds only the connection, its schema validation and the small
//! durable head.

use super::schema::{self, TABLE_HEAD, TABLE_META};
use super::{TIMELINE_SCHEMA_VERSION, TimelineStoreError};
use crate::studio::paths::sqlite_read_only_url;
use crate::studio::thread_projection::engine::ProjectionHead;
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
    Value,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// A bounded, read-only handle to one per-Thread session database's Studio timeline index.
#[derive(Debug)]
pub(crate) struct TimelineReader {
    pub(super) db: DatabaseConnection,
    pub(super) path: PathBuf,
}

/// Small durable head row used by page and fact planning.
pub(super) struct HeadRow {
    pub(super) watermark: u64,
    pub(super) next_ordinal: u64,
    pub(super) generation: String,
}

impl TimelineReader {
    /// Opens the session database read-only and validates the Studio index schema version.
    ///
    /// # Errors
    /// Fails when the file is missing, the index tables were never created, the schema version is
    /// unknown, or the read-only connection cannot be established. The database is never created.
    pub(crate) async fn open(path: impl AsRef<Path>) -> Result<Self, TimelineStoreError> {
        let path = path.as_ref().to_path_buf();
        if !tokio::fs::try_exists(&path).await? {
            return Err(TimelineStoreError::MissingDatabase { path });
        }
        let mut options = ConnectOptions::new(sqlite_read_only_url(&path));
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .sqlx_logging(false);
        let db = Database::connect(options).await?;
        let reader = Self { db, path };
        match reader.validate_schema().await {
            Ok(()) => Ok(reader),
            Err(error) => {
                let _ = reader.db.close().await;
                Err(error)
            }
        }
    }

    /// Drains and closes the bounded read-only connection.
    ///
    /// # Errors
    /// Returns the underlying database error if the handle could not be released.
    pub(crate) async fn close(self) -> Result<(), TimelineStoreError> {
        self.db.close().await?;
        Ok(())
    }

    async fn table_exists(&self, name: &str) -> Result<bool, TimelineStoreError> {
        let row = self
            .db
            .query_one_raw(statement(
                "SELECT 1 AS present FROM sqlite_schema WHERE type='table' AND name=?",
                vec![name.into()],
            ))
            .await?;
        Ok(row.is_some())
    }

    async fn meta_value(&self, key: &str) -> Result<Option<String>, TimelineStoreError> {
        let row = self
            .db
            .query_one_raw(statement(
                "SELECT value FROM timeline_meta WHERE key=?",
                vec![key.into()],
            ))
            .await?;
        row.map(|row| row.try_get::<String>("", "value"))
            .transpose()
            .map_err(Into::into)
    }

    async fn validate_schema(&self) -> Result<(), TimelineStoreError> {
        if !self.table_exists(TABLE_META).await? || !self.table_exists(TABLE_HEAD).await? {
            return Err(TimelineStoreError::IndexNotInitialized {
                path: self.path.clone(),
            });
        }
        let Some(value) = self.meta_value(schema::META_SCHEMA_KEY).await? else {
            return Err(TimelineStoreError::IndexNotInitialized {
                path: self.path.clone(),
            });
        };
        let found: i64 = value.parse().map_err(|_| {
            TimelineStoreError::Corrupt(format!(
                "timeline index schema marker `{value}` is not an integer"
            ))
        })?;
        if found != TIMELINE_SCHEMA_VERSION {
            return Err(TimelineStoreError::UnsupportedSchema {
                found,
                supported: TIMELINE_SCHEMA_VERSION,
            });
        }
        Ok(())
    }

    pub(super) async fn head_row(&self, thread_id: &str) -> Result<HeadRow, TimelineStoreError> {
        let sql = format!(
            "SELECT watermark, next_ordinal, generation FROM {TABLE_HEAD} WHERE thread_id=?"
        );
        let row = self
            .db
            .query_one_raw(statement(&sql, vec![thread_id.into()]))
            .await?;
        let row = row.ok_or_else(|| TimelineStoreError::ThreadNotIndexed {
            thread_id: thread_id.to_owned(),
        })?;
        Ok(HeadRow {
            watermark: unsigned(row.try_get::<i64>("", "watermark")?)?,
            next_ordinal: unsigned(row.try_get::<i64>("", "next_ordinal")?)?,
            generation: row.try_get::<String>("", "generation")?,
        })
    }

    /// The small durable head of one Thread's timeline index.
    ///
    /// # Errors
    /// Fails when the Thread has no durable index row (never treated as an empty timeline).
    pub(crate) async fn read_head(
        &self,
        thread_id: &str,
    ) -> Result<ProjectionHead, TimelineStoreError> {
        let head = self.head_row(thread_id).await?;
        Ok(ProjectionHead {
            thread_id: thread_id.to_owned(),
            watermark: head.watermark,
            next_ordinal: head.next_ordinal,
        })
    }
}

pub(super) fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

pub(super) fn unsigned(value: i64) -> Result<u64, TimelineStoreError> {
    u64::try_from(value)
        .map_err(|_| TimelineStoreError::Corrupt(format!("negative counter {value}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::timeline_store::fixture;

    #[tokio::test]
    async fn reader_rejects_a_missing_file_and_an_uninitialised_index() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.sqlite");
        let error = TimelineReader::open(&missing).await.unwrap_err();
        assert!(matches!(error, TimelineStoreError::MissingDatabase { .. }));
        assert!(error.is_index_fault());

        let path = fixture::session_path(&dir);
        fixture::seed(&path, "thread", &[fixture::commit(1)]).await;
        let error = TimelineReader::open(&path).await.unwrap_err();
        assert!(
            matches!(error, TimelineStoreError::IndexNotInitialized { .. }),
            "an unindexed session must never look like an empty timeline: {error:?}"
        );

        fixture::indexed(&path, "thread", &[fixture::commit(1)]).await;
        let reader = TimelineReader::open(&path).await.unwrap();
        assert_eq!(reader.read_head("thread").await.unwrap().watermark, 1);
        let error = reader.read_head("other").await.unwrap_err();
        assert!(matches!(error, TimelineStoreError::ThreadNotIndexed { .. }));
        reader.close().await.unwrap();
    }
}

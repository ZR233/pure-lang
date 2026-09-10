//! Restartable session-format reset, before either runtime database is published.
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
};
use serde::{Deserialize, Serialize};

use super::{paths, runtime_lock::RuntimeLock, store_support};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Preparing,
    BackedUp,
    Archived,
    ProductReady,
    Complete,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reset {
    version: u32,
    product_file: String,
    backup_directory: String,
    product_exists: bool,
    sessions_exist: bool,
    phase: Phase,
}

/// The live lock borrow restricts destructive coordination to the unpublished startup owner.
pub(super) async fn prepare(product: &Path, _owner: &RuntimeLock) -> Result<()> {
    let parent = product.parent().context("product database has no parent")?;
    let sessions = parent.join("sessions.sqlite");
    let marker = parent.join("session-reset.json");
    let mut session_lock = Some(lock_sessions(&sessions).await?);
    let mut reset = if tokio::fs::try_exists(&marker).await? {
        regular_file(&marker).await?;
        let reset: Reset = serde_json::from_slice(&tokio::fs::read(&marker).await?)?;
        ensure!(
            reset.version == 1,
            "unsupported session reset marker; recovery preserved"
        );
        ensure!(
            product.file_name().and_then(|name| name.to_str()) == Some(reset.product_file.as_str()),
            "session reset belongs to another database"
        );
        let mut components = Path::new(&reset.backup_directory).components();
        ensure!(
            matches!(components.next(), Some(Component::Normal(_)))
                && components.next().is_none()
                && reset.backup_directory.starts_with("session-backup-"),
            "invalid session backup directory"
        );
        reset
    } else {
        let product_version = inspect(product).await?;
        let session_version = inspect(&sessions).await?;
        if let Some(version) = product_version {
            ensure!(
                matches!(version, 19 | 20),
                "unsupported Studio schema {version}; existing data preserved"
            );
        }
        if let Some(version) = session_version {
            ensure!(
                (0..=pl_core::persistence::SESSION_SCHEMA_VERSION).contains(&version),
                "unsupported session schema {version}; existing data preserved"
            );
        }
        if product_version != Some(19)
            && session_version
                .is_none_or(|version| version == pl_core::persistence::SESSION_SCHEMA_VERSION)
        {
            return Ok(());
        }
        let backup_parent = parent.to_path_buf();
        let directory = tokio::task::spawn_blocking(move || {
            tempfile::Builder::new()
                .prefix("session-backup-")
                .tempdir_in(backup_parent)
                .map(tempfile::TempDir::keep)
        })
        .await??;
        let reset = Reset {
            version: 1,
            product_file: product
                .file_name()
                .and_then(|name| name.to_str())
                .context("non-UTF8 product database filename")?
                .to_owned(),
            backup_directory: directory
                .file_name()
                .and_then(|name| name.to_str())
                .context("non-UTF8 backup directory")?
                .to_owned(),
            product_exists: product_version.is_some(),
            sessions_exist: session_version.is_some(),
            phase: Phase::Preparing,
        };
        save_marker(&marker, &reset).await?;
        reset
    };
    let backup = parent.join(&reset.backup_directory);
    let metadata = tokio::fs::symlink_metadata(&backup).await?;
    ensure!(
        metadata.is_dir() && !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata),
        "session backup is not an owned directory"
    );
    loop {
        match reset.phase {
            Phase::Preparing => {
                if reset.product_exists {
                    backup_database(product, &backup.join("product.sqlite")).await?;
                }
                if reset.sessions_exist {
                    backup_database(&sessions, &backup.join("sessions.sqlite")).await?;
                }
                reset.phase = Phase::BackedUp;
            }
            Phase::BackedUp => {
                if reset.sessions_exist {
                    archive_family(&sessions, &backup).await?;
                }
                reset.phase = Phase::Archived;
            }
            Phase::Archived => {
                if reset.product_exists {
                    reset_product(product).await?;
                }
                reset.phase = Phase::ProductReady;
            }
            Phase::ProductReady => {
                // The fresh core owner obtains this same lock before opening its database.
                // RuntimeLock continues to exclude every other Studio startup.
                drop(session_lock.take());
                let store = pl_core::persistence::SqliteSessionStore::open(
                    pl_core::persistence::SqliteSessionOptions {
                        path: sessions.clone(),
                    },
                )
                .await?;
                store
                    .shutdown()
                    .await
                    .map_err(|error| anyhow::anyhow!(error))?;
                reset.phase = Phase::Complete;
            }
            Phase::Complete => {
                // A complete marker stays in the backup; its removal is the publication boundary.
                save_marker(&backup.join("reset.json"), &reset).await?;
                tokio::fs::remove_file(&marker).await?;
                sync_directory(parent).await?;
                return Ok(());
            }
        }
        save_marker(&marker, &reset).await?;
    }
}

async fn lock_sessions(path: &Path) -> Result<std::fs::File> {
    let path = path.with_extension("sqlite.lock");
    tokio::task::spawn_blocking(move || {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        fs4::FileExt::try_lock(&file).context("session database is owned; reset postponed")?;
        Ok(file)
    })
    .await?
}

async fn regular_file(path: &Path) -> Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    ensure!(
        metadata.is_file() && !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata),
        "database/reset target is not a regular file: {}",
        path.display()
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum DatabaseAccess {
    ReadOnly,
    ReadWrite,
}

async fn connect(path: &Path, access: DatabaseAccess) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(match access {
        DatabaseAccess::ReadOnly => paths::sqlite_read_only_url(path),
        DatabaseAccess::ReadWrite => paths::sqlite_url(path),
    });
    options
        .max_connections(1)
        .min_connections(1)
        .sqlx_logging(false);
    Ok(Database::connect(options).await?)
}

async fn inspect(path: &Path) -> Result<Option<i64>> {
    if !tokio::fs::try_exists(path).await? {
        for suffix in ["-wal", "-shm"] {
            ensure!(
                !tokio::fs::try_exists(sidecar(path, suffix)).await?,
                "orphan SQLite sidecar requires recovery"
            );
        }
        return Ok(None);
    }
    regular_file(path).await?;
    let db = connect(path, DatabaseAccess::ReadOnly).await?;
    let result = async {
        let rows = db.query_all_raw(sql("PRAGMA quick_check")).await?;
        let results = rows
            .into_iter()
            .map(|row| row.try_get::<String>("", "quick_check"))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            results.as_slice() == ["ok"],
            "corrupt database preserved: {}",
            results.join("; ")
        );
        let row = db
            .query_one_raw(sql("PRAGMA user_version"))
            .await?
            .context("missing SQLite version")?;
        Ok(Some(row.try_get::<i64>("", "user_version")?))
    }
    .await;
    finish_connection(db, result).await
}

async fn backup_database(source: &Path, destination: &Path) -> Result<()> {
    if tokio::fs::try_exists(destination).await? {
        inspect(destination)
            .await?
            .context("missing completed backup")?;
        return Ok(());
    }
    let partial = destination.with_extension("partial");
    if tokio::fs::try_exists(&partial).await? {
        regular_file(&partial).await?;
        tokio::fs::remove_file(&partial).await?;
    }
    regular_file(source).await?;
    let db = connect(source, DatabaseAccess::ReadWrite).await?;
    let result = async {
        let checkpoint = db
            .query_one_raw(sql("PRAGMA wal_checkpoint(TRUNCATE)"))
            .await?
            .context("missing WAL checkpoint result")?;
        ensure!(
            checkpoint.try_get::<i64>("", "busy")? == 0,
            "SQLite checkpoint is busy; reset postponed"
        );
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "VACUUM INTO ?",
            [partial.to_str().context("non-UTF8 backup path")?.into()],
        ))
        .await?;
        Ok(())
    }
    .await;
    finish_connection(db, result).await?;
    tokio::fs::File::open(&partial).await?.sync_all().await?;
    tokio::fs::rename(&partial, destination).await?;
    sync_directory(destination.parent().context("backup has no parent")?).await
}

async fn archive_family(source: &Path, backup: &Path) -> Result<()> {
    // Each move is individually resumable. Never overwrite a new or ambiguous source.
    for (suffix, name) in [
        ("", "original-sessions.sqlite"),
        ("-wal", "original-sessions.sqlite-wal"),
        ("-shm", "original-sessions.sqlite-shm"),
    ] {
        let source = sidecar(source, suffix);
        let destination = backup.join(name);
        let exists = tokio::fs::try_exists(&source).await?;
        let archived = tokio::fs::try_exists(&destination).await?;
        ensure!(
            !(exists && archived),
            "ambiguous session archive; both copies preserved"
        );
        if exists {
            regular_file(&source).await?;
            tokio::fs::rename(&source, &destination).await?;
        } else if suffix.is_empty() {
            ensure!(archived, "session database disappeared before archive");
        }
    }
    sync_directory(backup).await?;
    sync_directory(source.parent().context("session database has no parent")?).await
}

async fn reset_product(path: &Path) -> Result<()> {
    use sea_orm::TransactionTrait;
    let db = connect(path, DatabaseAccess::ReadWrite).await?;
    let result = async {
        store_support::upgrade_session_storage(&db).await?;
        let transaction = db.begin().await?;
        transaction.execute_unprepared("DELETE FROM studio_objects WHERE object_kind IN ('agentWorkingState','commitReceipt','modelPerformance'); DELETE FROM threads;").await?;
        transaction.commit().await?;
        Ok(())
    }.await;
    finish_connection(db, result).await
}

async fn finish_connection<T>(db: DatabaseConnection, result: Result<T>) -> Result<T> {
    match (result, db.close().await) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("failed to close reset database"),
        (Err(error), Err(close)) => {
            Err(error).context(format!("reset connection cleanup also failed: {close}"))
        }
    }
}

fn sql(value: &str) -> Statement {
    Statement::from_string(DatabaseBackend::Sqlite, value)
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

async fn save_marker(path: &Path, reset: &Reset) -> Result<()> {
    let path = path.to_owned();
    let encoded = serde_json::to_vec(reset)?;
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let parent = path.parent().context("reset marker has no parent")?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&encoded)?;
        file.as_file().sync_all()?;
        file.persist(&path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok::<_, anyhow::Error>(())
    })
    .await?
}

async fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all()).await??;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    async fn fixture(parent: &Path) -> PathBuf {
        let product = parent.join("studio.sqlite");
        let db = connect(&product, DatabaseAccess::ReadWrite).await.unwrap();
        store_support::initialize_studio_schema(&db).await.unwrap();
        db.execute_unprepared("INSERT INTO studio_objects VALUES ('product','keep','futurePlugin',1,99,'{\"unknown\":true}','unchanged',0); INSERT INTO studio_objects VALUES ('thread','old','agentWorkingState',1,1,'{}','old',0);").await.unwrap();
        db.close().await.unwrap();
        let db = connect(&parent.join("sessions.sqlite"), DatabaseAccess::ReadWrite)
            .await
            .unwrap();
        db.execute_unprepared("CREATE TABLE old_history(payload TEXT); INSERT INTO old_history VALUES ('original history'); PRAGMA user_version=4;").await.unwrap();
        db.close().await.unwrap();
        tokio::fs::write(parent.join("credentials.toml"), "secret placeholder")
            .await
            .unwrap();
        tokio::fs::create_dir(parent.join("workspace"))
            .await
            .unwrap();
        tokio::fs::write(parent.join("workspace/user.txt"), "user bytes")
            .await
            .unwrap();
        product
    }

    async fn assert_reset(parent: &Path, product: &Path) {
        assert_eq!(
            inspect(&parent.join("sessions.sqlite")).await.unwrap(),
            Some(pl_core::persistence::SESSION_SCHEMA_VERSION)
        );
        assert!(!parent.join("session-reset.json").exists());
        let db = connect(product, DatabaseAccess::ReadOnly).await.unwrap();
        let rows = db
            .query_all_raw(sql(
                "SELECT object_kind,payload_json FROM studio_objects ORDER BY object_kind",
            ))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].try_get::<String>("", "object_kind").unwrap(),
            "futurePlugin"
        );
        assert_eq!(
            rows[0].try_get::<String>("", "payload_json").unwrap(),
            "{\"unknown\":true}"
        );
        db.close().await.unwrap();
        assert_eq!(
            tokio::fs::read_to_string(parent.join("credentials.toml"))
                .await
                .unwrap(),
            "secret placeholder"
        );
        assert_eq!(
            tokio::fs::read_to_string(parent.join("workspace/user.txt"))
                .await
                .unwrap(),
            "user bytes"
        );
    }

    #[tokio::test]
    async fn reset_backs_up_history_and_preserves_unknown_product_data_and_user_files() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path();
        let product = fixture(parent).await;
        let lock = RuntimeLock::acquire(
            &parent.join("runtime.lock"),
            super::super::runtime_lock::StudioHostKind::Test,
        )
        .unwrap();
        prepare(&product, &lock).await.unwrap();
        assert_reset(parent, &product).await;
        let backup = std::fs::read_dir(parent)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("session-backup-")
            })
            .unwrap();
        let db = connect(&backup.join("sessions.sqlite"), DatabaseAccess::ReadOnly)
            .await
            .unwrap();
        let row = db
            .query_one_raw(sql("SELECT payload FROM old_history"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.try_get::<String>("", "payload").unwrap(),
            "original history"
        );
        db.close().await.unwrap();
        assert!(backup.join("product.sqlite").is_file());
        prepare(&product, &lock).await.unwrap();
        assert_reset(parent, &product).await;
    }

    #[tokio::test]
    async fn interrupted_archive_resumes_without_overwriting_its_saved_database() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path();
        let product = fixture(parent).await;
        let backup = parent.join("session-backup-interrupted");
        tokio::fs::create_dir(&backup).await.unwrap();
        backup_database(&product, &backup.join("product.sqlite"))
            .await
            .unwrap();
        backup_database(
            &parent.join("sessions.sqlite"),
            &backup.join("sessions.sqlite"),
        )
        .await
        .unwrap();
        archive_family(&parent.join("sessions.sqlite"), &backup)
            .await
            .unwrap();
        let original = tokio::fs::read(backup.join("original-sessions.sqlite"))
            .await
            .unwrap();
        save_marker(
            &parent.join("session-reset.json"),
            &Reset {
                version: 1,
                product_file: "studio.sqlite".into(),
                backup_directory: "session-backup-interrupted".into(),
                product_exists: true,
                sessions_exist: true,
                phase: Phase::BackedUp,
            },
        )
        .await
        .unwrap();
        let lock = RuntimeLock::acquire(
            &parent.join("runtime.lock"),
            super::super::runtime_lock::StudioHostKind::Test,
        )
        .unwrap();
        prepare(&product, &lock).await.unwrap();
        assert_reset(parent, &product).await;
        assert_eq!(
            tokio::fs::read(backup.join("original-sessions.sqlite"))
                .await
                .unwrap(),
            original
        );
    }

    #[tokio::test]
    async fn independently_owned_sessions_prevent_reset_before_backups_or_product_changes() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path();
        let product = fixture(parent).await;
        let other_owner = lock_sessions(&parent.join("sessions.sqlite"))
            .await
            .unwrap();
        let lock = RuntimeLock::acquire(
            &parent.join("runtime.lock"),
            super::super::runtime_lock::StudioHostKind::Test,
        )
        .unwrap();
        assert!(prepare(&product, &lock).await.is_err());
        assert!(!parent.join("session-reset.json").exists());
        assert_eq!(
            inspect(&parent.join("sessions.sqlite")).await.unwrap(),
            Some(4)
        );
        drop(other_owner);
        prepare(&product, &lock).await.unwrap();
        assert_reset(parent, &product).await;
    }
}

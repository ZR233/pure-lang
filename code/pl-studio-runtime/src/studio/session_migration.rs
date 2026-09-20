//! Non-destructive, offline session migration before either runtime database is published.
use super::{paths, runtime_lock::RuntimeLock, store_support};
use anyhow::{Context, Result, ensure};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
};
use std::path::{Path, PathBuf};

pub(super) async fn prepare(product: &Path, _owner: &RuntimeLock) -> Result<()> {
    let parent = product.parent().context("product database has no parent")?;
    let sessions = parent.join("sessions.sqlite");
    let _sessions_lock = lock_sessions(&sessions).await?;
    ensure!(
        !tokio::fs::try_exists(parent.join("session-reset.json")).await?,
        "unfinished legacy destructive reset requires recovery; all data preserved"
    );
    let product_version = inspect(product).await?;
    let session_version = inspect(&sessions).await?;
    ensure!(
        product_version.is_none_or(|v| matches!(v, 20..=22)),
        "unsupported Studio migration path; existing data preserved"
    );
    ensure!(
        session_version.is_none_or(|v| matches!(v, 6 | 7)),
        "unsupported session migration path; existing data preserved"
    );
    if session_version == Some(6) {
        let backup_parent = parent.to_path_buf();
        let backup = tokio::task::spawn_blocking(move || {
            tempfile::Builder::new()
                .prefix("session-backup-")
                .tempdir_in(backup_parent)
                .map(tempfile::TempDir::keep)
        })
        .await??;
        backup_database(&sessions, &backup.join("sessions.sqlite")).await?;
        if product_version.is_some() {
            backup_database(product, &backup.join("product.sqlite")).await?;
        }
        pl_core::persistence::migration::migrate_v6(
            pl_core::persistence::SqliteSessionOptions { path: sessions },
            super::collaboration_migration::convert,
        )
        .await?;
    }
    if matches!(product_version, Some(20 | 21)) {
        migrate_product(product, parent).await?;
    }
    // Split the legacy aggregate into per-Thread databases and publish the layout
    // marker. A fresh install without a legacy aggregate is a no-op.
    super::session_layout::migrate_layout(product, parent).await?;
    Ok(())
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
        fs4::FileExt::try_lock(&file).context("session database is owned; migration postponed")?;
        Ok(file)
    })
    .await?
}

async fn regular_file(path: &Path) -> Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    ensure!(
        metadata.is_file() && !pl_tool::workspace::path_safety::is_link_or_reparse(&metadata),
        "database migration target is not a regular file: {}",
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

pub(in crate::studio) async fn backup_database(source: &Path, destination: &Path) -> Result<()> {
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
            "SQLite checkpoint is busy; migration postponed"
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
    // Windows requires a writable handle for `FlushFileBuffers`, which backs
    // `sync_all`; opening the SQLite-created file read-only returns
    // `ERROR_ACCESS_DENIED` there.
    let partial_file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&partial)
        .await?;
    partial_file.sync_all().await?;
    drop(partial_file);
    tokio::fs::rename(&partial, destination).await?;
    sync_directory(destination.parent().context("backup has no parent")?).await
}

/// v20→v21 产品迁移：先备份产品库，再依次执行幂等的会话工作区模式升级与 SSH 服务器迁出。
async fn migrate_product(product: &Path, parent: &Path) -> Result<()> {
    let backup_parent = parent.to_path_buf();
    let backup = tokio::task::spawn_blocking(move || {
        tempfile::Builder::new()
            .prefix("product-backup-")
            .tempdir_in(backup_parent)
            .map(tempfile::TempDir::keep)
    })
    .await??;
    backup_database(product, &backup.join("product.sqlite")).await?;
    let db = connect(product, DatabaseAccess::ReadWrite).await?;
    let result = async {
        // 顺序固定：先做版本门控的 `threads.workspace_mode` 加列与 worktree lease 载荷转换
        // （必须仍处于 v20），再执行按 `ssh_servers` 表存在判定的 SSH 迁出。两者都幂等，
        // 崩溃后可按同一协调流重启续跑。
        store_support::upgrade_product_schema(&db).await?;
        super::store::ssh_migration::migrate_ssh_servers_to_user_config(
            &db,
            &pl_tool::remote::SshConfigFile::user_default()?,
        )
        .await
    }
    .await;
    finish_connection(db, result).await
}

async fn finish_connection<T>(db: DatabaseConnection, result: Result<T>) -> Result<T> {
    match (result, db.close().await) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("failed to close migration database"),
        (Err(error), Err(close)) => {
            Err(error).context(format!("migration connection cleanup also failed: {close}"))
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

pub(in crate::studio) async fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all()).await??;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

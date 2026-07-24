use anyhow::Result;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 3;
const BASE_SCHEMA: &str = include_str!("../../migrations/0001_base.sql");
const AGENT_SESSIONS_MIGRATION: &str = include_str!("../../migrations/0002_agent_sessions.sql");

pub(super) async fn configure_sqlite(db: &DatabaseConnection) -> Result<()> {
    for pragma in [
        "PRAGMA journal_mode=WAL",
        "PRAGMA synchronous=NORMAL",
        "PRAGMA busy_timeout=5000",
        "PRAGMA foreign_keys=ON",
    ] {
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            pragma.to_string(),
        ))
        .await?;
    }
    Ok(())
}

pub(super) async fn initialize_schema(db: &DatabaseConnection) -> Result<()> {
    let tx = db.begin().await?;
    for statement in split_sql(BASE_SCHEMA) {
        tx.execute(Statement::from_string(DatabaseBackend::Sqlite, statement))
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(super) async fn migrate_schema(db: &DatabaseConnection, from_version: i64) -> Result<()> {
    let tx = db.begin().await?;
    match from_version {
        2 => {
            for statement in split_sql(AGENT_SESSIONS_MIGRATION) {
                tx.execute(Statement::from_string(DatabaseBackend::Sqlite, statement))
                    .await?;
            }
            super::migration::split_v2_agent_sessions(&tx).await?;
        }
        version => anyhow::bail!(
            "不支持从 Studio 数据库版本 {version} 迁移到 {STUDIO_DATABASE_SCHEMA_VERSION}"
        ),
    }
    tx.commit().await?;
    Ok(())
}

pub(super) fn non_empty_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "新会话".to_string()
    } else {
        title.chars().take(80).collect()
    }
}

fn split_sql(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(|statement| format!("{statement};"))
        .collect()
}

use anyhow::Result;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 7;
const BASE_SCHEMA: &str = include_str!("../../migrations/0001_base.sql");
const AGENT_SESSIONS_MIGRATION: &str = include_str!("../../migrations/0002_agent_sessions.sql");
const TASK_COMPLETION_CONTRACT_MIGRATION: &str =
    include_str!("../../migrations/0003_task_completion_contract.sql");
const AGENT_WAKE_RECEIPTS_MIGRATION: &str =
    include_str!("../../migrations/0004_agent_wake_receipts.sql");
const WORKTREE_DISPOSITION_MIGRATION: &str =
    include_str!("../../migrations/0005_worktree_disposition.sql");
const MAILBOX_AND_TASK_STOP_MIGRATION: &str =
    include_str!("../../migrations/0006_mailbox_and_task_stop.sql");

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
    let result = async {
        let mut version = from_version;
        if version == 2 {
            execute_sql(&tx, AGENT_SESSIONS_MIGRATION).await?;
            super::migration::split_v2_agent_sessions(&tx).await?;
            version = 3;
        }
        if version == 3 {
            execute_sql(&tx, TASK_COMPLETION_CONTRACT_MIGRATION).await?;
            ensure_column(
                &tx,
                "task_runs",
                "stop_requested",
                "INTEGER NOT NULL DEFAULT 0",
            )
            .await?;
            ensure_column(&tx, "task_runs", "stop_requested_reason", "TEXT").await?;
            ensure_column(&tx, "task_runs", "stop_requested_at", "INTEGER").await?;
            ensure_column(&tx, "agent_outcomes", "completion_contract_json", "TEXT").await?;
            ensure_column(
                &tx,
                "agent_outcomes",
                "delivery_recovery_count",
                "INTEGER NOT NULL DEFAULT 0",
            )
            .await?;
            backfill_delivery_contracts(&tx).await?;
            version = 4;
        }
        if version == 4 {
            execute_sql(&tx, AGENT_WAKE_RECEIPTS_MIGRATION).await?;
            version = 5;
        }
        if version == 5 {
            execute_sql(&tx, WORKTREE_DISPOSITION_MIGRATION).await?;
            version = 6;
        }
        if version == 6 {
            execute_sql(&tx, MAILBOX_AND_TASK_STOP_MIGRATION).await?;
            backfill_terminal_generations(&tx).await?;
            version = 7;
        }
        if version != STUDIO_DATABASE_SCHEMA_VERSION {
            anyhow::bail!(
                "不支持从 Studio 数据库版本 {from_version} 迁移到 {STUDIO_DATABASE_SCHEMA_VERSION}"
            );
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            tx.commit().await?;
            Ok(())
        }
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
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

async fn execute_sql(connection: &impl ConnectionTrait, sql: &str) -> Result<()> {
    for statement in split_sql(sql) {
        connection
            .execute(Statement::from_string(DatabaseBackend::Sqlite, statement))
            .await?;
    }
    Ok(())
}

async fn ensure_column(
    connection: &impl ConnectionTrait,
    table: &str,
    column: &str,
    declaration: &str,
) -> Result<()> {
    if !table_has_column(connection, table, column).await? {
        connection
            .execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {declaration}"),
            ))
            .await?;
    }
    Ok(())
}

async fn table_has_column(
    connection: &impl ConnectionTrait,
    table: &str,
    column: &str,
) -> Result<bool> {
    let rows = connection
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info(\"{table}\")"),
        ))
        .await?;
    Ok(rows.iter().any(|row| {
        row.try_get::<String>("", "name")
            .is_ok_and(|stored| stored == column)
    }))
}

async fn backfill_terminal_generations(connection: &impl ConnectionTrait) -> Result<()> {
    if !table_has_column(connection, "task_runs", "phase").await? {
        return Ok(());
    }
    connection
        .execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "UPDATE task_runs
             SET terminal_generation = task_generation
             WHERE phase IN ('completed', 'blocked', 'failed', 'cancelled')"
                .to_string(),
        ))
        .await?;
    Ok(())
}

async fn backfill_delivery_contracts(connection: &impl ConnectionTrait) -> Result<()> {
    let rows = connection
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, task_run_id, work_unit_id
             FROM agent_outcomes
             WHERE role = 'executor'
               AND work_unit_id IS NOT NULL
               AND completion_contract_json IS NULL"
                .to_string(),
        ))
        .await?;
    for row in rows {
        let id = row.try_get::<String>("", "id")?;
        let task_run_id = row.try_get::<String>("", "task_run_id")?;
        let work_unit_id = row.try_get::<String>("", "work_unit_id")?;
        let contract = serde_json::to_string(&serde_json::json!({
            "kind": "deliveryRequired",
            "taskRunId": task_run_id,
            "workUnitId": work_unit_id,
            "recoveryLimit": 1,
        }))?;
        connection
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE agent_outcomes
                 SET completion_contract_json = ?
                 WHERE id = ? AND completion_contract_json IS NULL",
                [contract.into(), id.into()],
            ))
            .await?;
    }
    Ok(())
}

//! 已停止会话的显式删除；宿主不需要了解 SQLite 表、存储键或检查点。

use sea_orm::{ConnectionTrait, TransactionTrait};

use super::{SessionStoreError, SqliteSessionOptions, SqliteSessionStore, sqlite};

impl SqliteSessionStore {
    /// 从已有数据库中原子删除一个会话的资源、历史和 Thread 检查点。
    ///
    /// 宿主须先关闭该会话的 Thread 和原持久化句柄。此方法为维护操作独占数据库锁，
    /// 有正在运行的 writer 时会显式失败，不会与其竞争或静默丢弃在途提交。
    /// 其它会话仍留在同一个数据库中。数据库文件不存在或该会话无记录时返回 `false`。
    ///
    /// # Errors
    /// 拒绝空会话标识、无效路径、当前仍被持有的数据库、损坏的 schema 或删除事务失败。
    pub async fn delete_session(
        options: SqliteSessionOptions,
        session_id: &str,
    ) -> Result<bool, SessionStoreError> {
        if session_id.is_empty() {
            return Err(SessionStoreError::Invalid("session id is empty".into()));
        }
        if !options.path.is_absolute() {
            return Err(SessionStoreError::Invalid(
                "session database path must be absolute".into(),
            ));
        }
        if !tokio::fs::try_exists(&options.path).await? {
            return Ok(false);
        }
        let store = Self::open(options).await?;
        let deletion = async {
            let tx = store.owner.shared.db.begin().await?;
            let mut deleted = false;
            for (table, identity) in [
                ("thread_checkpoints", "thread_id"),
                ("session_entries", "session_id"),
                ("session_entry_history", "session_id"),
                ("session_history_heads", "session_id"),
            ] {
                let sql = format!("DELETE FROM {table} WHERE {identity}=?");
                let result = tx
                    .execute_raw(sqlite::statement(&sql, vec![session_id.into()]))
                    .await?;
                deleted |= result.rows_affected() > 0;
            }
            tx.commit().await?;
            Ok::<bool, SessionStoreError>(deleted)
        }
        .await;
        let shutdown = store
            .shutdown()
            .await
            .map_err(SessionStoreError::MaintenanceShutdown);
        match (deletion, shutdown) {
            (Ok(deleted), Ok(())) => Ok(deleted),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }
}

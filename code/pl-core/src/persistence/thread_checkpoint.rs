//! 已持久化的 Thread 当前状态 checkpoint，由 pl-core 解码并校验所有权与历史水位。
//!
//! checkpoint 与它引用的 effect 由同一个 writer 事务写入，所以读取时只要 effect 落在磁盘上，
//! checkpoint 的 `history_fence` 就一定指向一条已保存的 effect。宿主只通过
//! [`SqliteSessionStore::read_thread_checkpoint`] 读取，不查询原始表、键或存储信封。

use sea_orm::ConnectionTrait;

use super::{SessionStoreError, SqliteSessionStore, sqlite};
use crate::thread::ThreadCheckpoint;

/// effect 资源键前缀；checkpoint 的 history fence 必须有一条同名 effect。
const THREAD_COMMIT_PREFIX: &str = "pl.resource.thread-commit.";

impl SqliteSessionStore {
    /// 读取一个 Thread 的最新当前状态 checkpoint，或在从未保存过时返回 `None`。
    ///
    /// 返回的 checkpoint 是 core 保存的 pruned、inline 重启 DTO：正文全部内联，没有需要外部
    /// blob 才能补齐的引用，可直接交给 owner 恢复。pl-core 在这里验证存储信封、内容 hash、Thread
    /// 身份、checkpoint schema 版本、`state_revision`/`state.commit_sequence` 一致性、
    /// `history_fence` 与状态 revision 的关系，以及在 `history_fence` 处确实存在已保存的 effect。
    ///
    /// # Errors
    /// 拒绝损坏的信封、内容 hash 不匹配、Thread 身份不一致、schema 不支持、revision/fence 不一致，
    /// 或 fence 指向一条并不存在的 effect。
    pub async fn read_thread_checkpoint(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadCheckpoint>, SessionStoreError> {
        let row = self
            .owner
            .shared
            .db
            .query_one_raw(sqlite::statement(
                "SELECT * FROM thread_checkpoints WHERE thread_id=?",
                vec![thread_id.into()],
            ))
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let checkpoint = decode_checkpoint(row, thread_id)?;
        // checkpoint 与 effect 同事务写入，因此 fence 处必须已经有 effect；缺失说明存储被截断、
        // 篡改或来自不完整的旧快照，一律 fail closed，绝不返回一个引用未保存历史的 checkpoint。
        let effect_id = format!("{THREAD_COMMIT_PREFIX}{:020}", checkpoint.history_fence);
        let effect = self
            .owner
            .shared
            .db
            .query_one_raw(sqlite::statement(
                "SELECT id FROM session_entries WHERE session_id=? AND id=?",
                vec![thread_id.into(), effect_id.into()],
            ))
            .await?;
        if effect.is_none() {
            return Err(SessionStoreError::Invalid(
                "Thread checkpoint history fence has no saved effect".into(),
            ));
        }
        Ok(Some(checkpoint))
    }
}

fn decode_checkpoint(
    row: sea_orm::QueryResult,
    thread_id: &str,
) -> Result<ThreadCheckpoint, SessionStoreError> {
    let envelope: String = row.try_get("", "envelope")?;
    let hash: String = row.try_get("", "payload_hash")?;
    if crate::context::content_hash(envelope.as_bytes()) != hash {
        return Err(SessionStoreError::Invalid(
            "Thread checkpoint integrity check failed".into(),
        ));
    }
    let stored_thread: String = row.try_get("", "thread_id")?;
    let schema_version: i64 = row.try_get("", "schema_version")?;
    let state_revision = u64::try_from(row.try_get::<i64>("", "state_revision")?)
        .map_err(|_| SessionStoreError::Invalid("negative checkpoint revision".into()))?;
    let history_fence = u64::try_from(row.try_get::<i64>("", "history_fence")?)
        .map_err(|_| SessionStoreError::Invalid("negative checkpoint fence".into()))?;
    let checkpoint: ThreadCheckpoint = serde_json::from_str(&envelope)?;
    if stored_thread != thread_id || checkpoint.thread_id != thread_id {
        return Err(SessionStoreError::Invalid(
            "Thread checkpoint ownership mismatch".into(),
        ));
    }
    if !ThreadCheckpoint::supports_schema(checkpoint.schema_version)
        || i64::from(checkpoint.schema_version) != schema_version
    {
        return Err(SessionStoreError::Invalid(format!(
            "unsupported Thread checkpoint schema {}",
            checkpoint.schema_version
        )));
    }
    if checkpoint.state_revision != state_revision || checkpoint.history_fence != history_fence {
        return Err(SessionStoreError::Invalid(
            "Thread checkpoint envelope/index mismatch".into(),
        ));
    }
    if checkpoint.state_revision != checkpoint.state.commit_sequence
        || checkpoint.history_fence > checkpoint.state_revision
        || checkpoint.state.usage_summary.applied_sequence > checkpoint.state_revision
        || !checkpoint.external_bodies.is_empty()
    {
        return Err(SessionStoreError::Invalid(
            "Thread checkpoint state is inconsistent with its fence".into(),
        ));
    }
    Ok(checkpoint)
}

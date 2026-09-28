//! 已持久化的 Thread 效果历史，由 pl-core 解码并校验所有权。

use sea_orm::ConnectionTrait;

use super::{SessionStoreError, SqliteSessionStore, sqlite};
use crate::{context::OpaquePayload, thread::ThreadEffectBatch};

const PREFIX: &str = "pl.resource.thread-commit.";
const UPPER_BOUND: &str = "pl.resource.thread-commit/";
const MAX_PAGE_SIZE: usize = 256;

/// 按提交序号倒序查询一个 Thread 的持久化效果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadEffectQuery {
    /// 排他上界；`None` 从最新提交开始。
    pub before_sequence: Option<u64>,
    /// 每页条数，范围为 1 到 256。
    pub limit: usize,
}

/// 一页已校验的 Thread 效果，按提交序号倒序排列。
#[derive(Debug)]
pub struct ThreadEffectPage {
    pub effects: Vec<ThreadEffectBatch>,
    /// 下一页的排他上界；`None` 表示已经到达历史起点。
    pub next_before_sequence: Option<u64>,
}

impl SqliteSessionStore {
    /// 按 Thread 标识和提交序号读取一条已持久化的效果。
    ///
    /// # Errors
    /// 拒绝损坏的存储信封、效果编码以及 Thread 所有权或序号不匹配的记录。
    pub async fn read_thread_effect(
        &self,
        thread_id: &str,
        sequence: u64,
    ) -> Result<Option<ThreadEffectBatch>, SessionStoreError> {
        let id = format!("{PREFIX}{sequence:020}");
        let row = self
            .owner
            .shared
            .db
            .query_one_raw(sqlite::statement(
                "SELECT * FROM session_entries WHERE session_id=? AND id=?",
                vec![thread_id.into(), id.into()],
            ))
            .await?;
        row.map(|row| decode_effect(row, thread_id)).transpose()
    }

    /// 查询一页已持久化的 Thread 效果。未写入完成的效果不会出现。
    ///
    /// # Errors
    /// 拒绝无效页大小、损坏的存储信封、效果编码及所有权或序号不匹配的记录。
    pub async fn query_thread_effects(
        &self,
        thread_id: &str,
        query: ThreadEffectQuery,
    ) -> Result<ThreadEffectPage, SessionStoreError> {
        if !(1..=MAX_PAGE_SIZE).contains(&query.limit) {
            return Err(SessionStoreError::Invalid(format!(
                "Thread effect page size must be 1..={MAX_PAGE_SIZE}"
            )));
        }
        let upper = query
            .before_sequence
            .map(|sequence| format!("{PREFIX}{sequence:020}"))
            .unwrap_or_else(|| UPPER_BOUND.into());
        let limit = i64::try_from(query.limit + 1)
            .map_err(|_| SessionStoreError::Invalid("Thread effect page size overflow".into()))?;
        let rows = self.owner.shared.db.query_all_raw(sqlite::statement(
            "SELECT * FROM session_entries WHERE session_id=? AND id>=? AND id<? ORDER BY id DESC LIMIT ?",
            vec![thread_id.into(), PREFIX.into(), upper.into(), limit.into()],
        )).await?;
        let mut effects = rows
            .into_iter()
            .map(|row| decode_effect(row, thread_id))
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = effects.len() > query.limit;
        if has_more {
            effects.pop();
        }
        let next_before_sequence = has_more.then(|| {
            effects
                .last()
                .expect("page with extra row has a retained item")
                .sequence
        });
        Ok(ThreadEffectPage {
            effects,
            next_before_sequence,
        })
    }
}

pub(super) fn decode_effect(
    row: sea_orm::QueryResult,
    thread_id: &str,
) -> Result<ThreadEffectBatch, SessionStoreError> {
    let entry = sqlite::decode_row(row)?;
    let payload = OpaquePayload::new(entry.type_id, entry.schema_version, entry.payload)
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
    let effect = ThreadEffectBatch::decode(&payload)
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
    if entry.session_id != thread_id
        || effect.thread_id != thread_id
        || entry.id != format!("{PREFIX}{:020}", effect.sequence)
    {
        return Err(SessionStoreError::Invalid(
            "Thread effect ownership or sequence key mismatch".into(),
        ));
    }
    Ok(effect)
}

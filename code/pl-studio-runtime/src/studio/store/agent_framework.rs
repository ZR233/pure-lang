use anyhow::Result;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::studio::StudioStore;

impl StudioStore {
    /// 查询 turn 的产品元数据；queue 行转为 running/terminal 时仍会保留该值。
    pub(in crate::studio) async fn agent_turn_metadata(
        &self,
        agent_id: &str,
        turn_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        let row = self
            .db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT metadata_json FROM agent_turns
                 WHERE agent_id = ? AND turn_id = ?",
                [agent_id.to_string().into(), turn_id.to_string().into()],
            ))
            .await?;
        row.map(|row| {
            let metadata: Option<String> = row.try_get("", "metadata_json")?;
            metadata
                .map(|metadata| serde_json::from_str(&metadata))
                .transpose()
                .map_err(Into::into)
        })
        .transpose()
        .map(Option::flatten)
    }

    /// 判断同一 task run 是否已经在 framework durable FIFO 中排有后续轮。
    ///
    /// 正在执行的 turn 不能阻止追加下一轮：child 可能先于当前 root turn 结束，
    /// 此时必须把 continuation 排到当前 turn 之后，避免丢失唤醒。
    pub(in crate::studio) async fn has_queued_task_continuation(
        &self,
        task_run_id: &str,
    ) -> Result<bool> {
        let row = self
            .db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS present
                 FROM agent_turns
                 WHERE status = 'queued'
                   AND json_extract(metadata_json, '$.taskRunId') = ?
                 LIMIT 1",
                [task_run_id.to_string().into()],
            ))
            .await?;
        Ok(row.is_some())
    }
}

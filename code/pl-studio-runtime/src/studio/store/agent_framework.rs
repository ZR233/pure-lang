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

    /// 判断同一 task run 是否已有排队或正在执行的受管续轮。
    ///
    /// 普通 root turn 即使正在执行也不能阻止 child terminal 追加下一轮；只有
    /// `historyPolicy=ephemeral` 的受管续轮在 live 状态时参与去重。
    pub(in crate::studio) async fn has_live_task_continuation(
        &self,
        task_run_id: &str,
    ) -> Result<bool> {
        let row = self
            .db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS present
                 FROM agent_turns
                 WHERE json_extract(metadata_json, '$.taskRunId') = ?
                   AND (
                     status = 'queued'
                     OR (
                       status IN ('running', 'waiting_tool', 'waiting_interaction')
                       AND json_extract(metadata_json, '$.historyPolicy') = 'ephemeral'
                     )
                   )
                 LIMIT 1",
                [task_run_id.to_string().into()],
            ))
            .await?;
        Ok(row.is_some())
    }
}

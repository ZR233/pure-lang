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
}

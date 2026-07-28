use std::collections::BTreeSet;

use anyhow::Result;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::studio::StudioStore;

pub(in crate::studio) struct RecoverableTaskPlan {
    pub agent_id: String,
    pub session_id: String,
    pub plan: pl_trace::TracePart,
}

impl StudioStore {
    /// 返回每个活动 Task 根会话最新的完整 Plan trace，供启动时修复缺失的确认投影。
    pub(in crate::studio) async fn list_latest_task_plan_traces(
        &self,
    ) -> Result<Vec<RecoverableTaskPlan>> {
        let rows = self
            .db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT session.id AS session_id, session.owner_agent_id AS agent_id,
                        trace.payload_json AS payload_json
                 FROM sessions session
                 INNER JOIN agent_runtime_traces trace ON trace.session_id = session.id
                 WHERE session.mode = 'task'
                   AND session.archived = 0
                   AND session.visibility = 'active'
                   AND session.session_kind = 'root'
                 ORDER BY session.id, trace.sequence DESC"
                    .to_string(),
            ))
            .await?;
        let mut recovered_sessions = BTreeSet::new();
        let mut plans = Vec::new();
        for row in rows {
            let session_id: String = row.try_get("", "session_id")?;
            if recovered_sessions.contains(&session_id) {
                continue;
            }
            let payload: String = row.try_get("", "payload_json")?;
            let trace: pl_trace::TraceEvent = match serde_json::from_str(&payload) {
                Ok(trace) => trace,
                Err(error) => {
                    tracing::warn!(
                        session_id,
                        %error,
                        "skipping malformed historical agent trace during plan recovery"
                    );
                    continue;
                }
            };
            let pl_trace::TraceEventKind::TracePartCompleted { item } = trace.kind else {
                continue;
            };
            if item.kind != pl_trace::TracePartKind::Plan || item.content.trim().is_empty() {
                continue;
            }
            let agent_id = row.try_get("", "agent_id")?;
            recovered_sessions.insert(session_id.clone());
            plans.push(RecoverableTaskPlan {
                agent_id,
                session_id,
                plan: item,
            });
        }
        Ok(plans)
    }

    pub(in crate::studio) async fn read_session_view_snapshot(
        &self,
        session_id: &str,
    ) -> Result<Option<pl_protocol::SessionViewSnapshot>> {
        self.db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT snapshot_json FROM session_view_snapshots WHERE session_id = ?",
                [session_id.to_string().into()],
            ))
            .await?
            .map(|row| {
                serde_json::from_str(&row.try_get::<String>("", "snapshot_json")?)
                    .map_err(Into::into)
            })
            .transpose()
    }

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

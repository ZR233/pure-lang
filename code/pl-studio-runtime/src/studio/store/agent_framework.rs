use std::collections::{HashMap, HashSet};

use anyhow::Result;
use pl_protocol::{SessionEventEnvelope, SessionEventKind, SessionPartContent, SessionPartStatus};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, TransactionTrait};

use crate::studio::StudioStore;
use crate::studio::entity::{
    agent_active_input, agent_outcome, agent_pending_input, agent_runtime_session,
    agent_runtime_state, agent_turn, session, session_history_item, session_view_snapshot,
    work_unit,
};

pub(in crate::studio) struct RecoverablePlan {
    pub turn_id: String,
    pub item_id: String,
    pub content: String,
}

pub(in crate::studio) struct RecoverableTaskPlan {
    pub agent_id: String,
    pub session_id: String,
    pub plan: RecoverablePlan,
}

impl StudioStore {
    pub(in crate::studio) async fn list_task_agent_snapshots(
        &self,
        task_run_id: &str,
    ) -> Result<Vec<pl_core::AgentSnapshot>> {
        let agent_ids = agent_outcome::Entity::find()
            .filter(agent_outcome::Column::TaskRunId.eq(task_run_id))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|outcome| outcome.agent_id)
            .collect::<HashSet<_>>();
        if agent_ids.is_empty() {
            return Ok(Vec::new());
        }
        agent_runtime_state::Entity::find()
            .filter(agent_runtime_state::Column::AgentId.is_in(agent_ids))
            .order_by_asc(agent_runtime_state::Column::AgentId)
            .all(&self.db)
            .await?
            .into_iter()
            .map(|state| serde_json::from_str(&state.snapshot_json).map_err(Into::into))
            .collect()
    }

    /// 清除严格可证明不再拥有任何 Studio session 的错误 runtime registration。
    ///
    /// 共享的 session journal/snapshot 不属于 runtime agent，不能在这里删除。
    pub(in crate::studio) async fn reconcile_runtime_session_ownership(
        &self,
    ) -> Result<Vec<String>> {
        let tx = self.db.begin().await?;
        let states = agent_runtime_state::Entity::find()
            .order_by_asc(agent_runtime_state::Column::AgentId)
            .all(&tx)
            .await?;
        let sessions = session::Entity::find().all(&tx).await?;
        let sessions_by_id = sessions
            .iter()
            .map(|session| (session.id.clone(), session.owner_agent_id.clone()))
            .collect::<HashMap<_, _>>();
        let session_owners = sessions
            .into_iter()
            .map(|session| session.owner_agent_id)
            .collect::<HashSet<_>>();
        let outcome_agents = agent_outcome::Entity::find()
            .all(&tx)
            .await?
            .into_iter()
            .map(|outcome| outcome.agent_id)
            .collect::<HashSet<_>>();
        let work_unit_agents = work_unit::Entity::find()
            .all(&tx)
            .await?
            .into_iter()
            .filter_map(|unit| unit.agent_id)
            .collect::<HashSet<_>>();
        let claims = agent_runtime_session::Entity::find()
            .all(&tx)
            .await?
            .into_iter()
            .map(|claim| (claim.agent_id, claim.session_id))
            .collect::<HashMap<_, _>>();
        let agent_ids = states
            .into_iter()
            .map(|state| state.agent_id)
            .filter(|agent_id| {
                !session_owners.contains(agent_id)
                    && !outcome_agents.contains(agent_id)
                    && !work_unit_agents.contains(agent_id)
                    && claims.get(agent_id).is_some_and(|session_id| {
                        sessions_by_id
                            .get(session_id)
                            .is_some_and(|owner_agent_id| owner_agent_id != agent_id)
                    })
            })
            .collect::<Vec<_>>();

        for agent_id in &agent_ids {
            agent_active_input::Entity::delete_by_id(agent_id.clone())
                .exec(&tx)
                .await?;
            agent_pending_input::Entity::delete_many()
                .filter(agent_pending_input::Column::AgentId.eq(agent_id))
                .exec(&tx)
                .await?;
            agent_turn::Entity::delete_many()
                .filter(agent_turn::Column::AgentId.eq(agent_id))
                .exec(&tx)
                .await?;
            agent_runtime_session::Entity::delete_by_id(agent_id.clone())
                .exec(&tx)
                .await?;
            agent_runtime_state::Entity::delete_by_id(agent_id.clone())
                .exec(&tx)
                .await?;
        }
        tx.commit().await?;
        Ok(agent_ids)
    }

    /// 返回每个活动 Task 根会话最新的完整 Plan，供启动时修复缺失的确认投影。
    pub(in crate::studio) async fn list_latest_task_plan_traces(
        &self,
    ) -> Result<Vec<RecoverableTaskPlan>> {
        let sessions = session::Entity::find()
            .filter(session::Column::Mode.eq("task"))
            .filter(session::Column::Archived.eq(0))
            .filter(session::Column::Visibility.eq("active"))
            .filter(session::Column::SessionKind.eq("root"))
            .order_by_asc(session::Column::Id)
            .all(&self.db)
            .await?;
        let mut plans = Vec::new();
        for session in sessions {
            let session_id = session.id;
            let history = session_history_item::Entity::find()
                .filter(session_history_item::Column::SessionId.eq(session_id.clone()))
                .filter(session_history_item::Column::ItemKind.eq("partChanged"))
                .order_by_desc(session_history_item::Column::Sequence)
                .all(&self.history_db)
                .await?;
            let plan = history.into_iter().find_map(|item| {
                let event: SessionEventEnvelope = match serde_json::from_str(&item.payload_json) {
                    Ok(event) => event,
                    Err(error) => {
                        tracing::warn!(
                            session_id = %session_id,
                            sequence = item.sequence,
                            error_bytes = error.to_string().len(),
                            "skipping malformed history item during plan recovery"
                        );
                        return None;
                    }
                };
                let SessionEventKind::PartChanged { part } = event.kind else {
                    return None;
                };
                let SessionPartContent::Plan { content } = part.content else {
                    return None;
                };
                if part.status != SessionPartStatus::Completed || content.trim().is_empty() {
                    return None;
                }
                Some(RecoverablePlan {
                    turn_id: part.turn_id,
                    item_id: part.part_id,
                    content,
                })
            });
            let Some(plan) = plan else {
                continue;
            };
            plans.push(RecoverableTaskPlan {
                agent_id: session.owner_agent_id,
                session_id,
                plan,
            });
        }
        Ok(plans)
    }

    pub(in crate::studio) async fn read_session_view_snapshot(
        &self,
        session_id: &str,
    ) -> Result<Option<pl_protocol::SessionViewSnapshot>> {
        session_view_snapshot::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
            .map(|snapshot| serde_json::from_str(&snapshot.snapshot_json).map_err(Into::into))
            .transpose()
    }

    /// 查询 turn 的产品元数据；queue 行转为 running/terminal 时仍会保留该值。
    pub(in crate::studio) async fn agent_turn_metadata(
        &self,
        agent_id: &str,
        turn_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        agent_turn::Entity::find_by_id((agent_id.to_string(), turn_id.to_string()))
            .one(&self.db)
            .await?
            .map(|turn| {
                turn.metadata_json
                    .map(|metadata| serde_json::from_str(&metadata))
                    .transpose()
                    .map_err(Into::into)
            })
            .transpose()
            .map(Option::flatten)
    }
}

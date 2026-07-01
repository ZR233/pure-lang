use anyhow::Result;
use pl_protocol::{StudioAgentTimelineEvent, StudioAgentTimelineEventKind};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter,
    QueryOrder,
};

use crate::studio::entities;
use crate::studio::mappers::{agent_snapshot_record, agent_timeline_event_record};
use crate::studio::records::{AgentSnapshotRecord, AgentTimelineEventRecord};
use crate::studio::store::StudioStore;

impl StudioStore {
    pub async fn upsert_agent_snapshot(&self, record: AgentSnapshotRecord) -> Result<()> {
        use entities::agent;
        if let Some(existing) = agent::Entity::find_by_id(record.id.clone())
            .one(&self.db)
            .await?
        {
            let mut active: agent::ActiveModel = existing.into();
            active.session_id = Set(record.session_id);
            active.path = Set(record.path);
            active.parent_path = Set(record.parent_path);
            active.role = Set(record.role);
            active.task = Set(record.task);
            active.status = Set(record.status.as_str().to_string());
            active.summary = Set(record.summary);
            active.error = Set(record.error);
            active.reason = Set(record.reason);
            active.budget_limit_kind = Set(record
                .budget_limit_kind
                .map(|kind| kind.as_str().to_string()));
            active.budget_usage_json = Set(record
                .budget_usage
                .and_then(|usage| serde_json::to_string(&usage).ok()));
            active.depth = Set(record.depth);
            active.updated_at = Set(record.updated_at);
            active.update(&self.db).await?;
        } else {
            agent::ActiveModel {
                id: Set(record.id),
                session_id: Set(record.session_id),
                path: Set(record.path),
                parent_path: Set(record.parent_path),
                role: Set(record.role),
                task: Set(record.task),
                status: Set(record.status.as_str().to_string()),
                summary: Set(record.summary),
                error: Set(record.error),
                reason: Set(record.reason),
                budget_limit_kind: Set(record
                    .budget_limit_kind
                    .map(|kind| kind.as_str().to_string())),
                budget_usage_json: Set(record
                    .budget_usage
                    .and_then(|usage| serde_json::to_string(&usage).ok())),
                depth: Set(record.depth),
                updated_at: Set(record.updated_at),
            }
            .insert(&self.db)
            .await?;
        }
        Ok(())
    }

    pub async fn list_agents(&self, session_id: &str) -> Result<Vec<AgentSnapshotRecord>> {
        use entities::agent;
        let rows = agent::Entity::find()
            .filter(agent::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(agent::Column::Path)
            .all(&self.db)
            .await?;
        let runtime_by_agent = self.agent_runtime_usage_by_agent(session_id).await?;
        Ok(rows
            .into_iter()
            .map(agent_snapshot_record)
            .map(|mut record| {
                record.runtime_usage = runtime_by_agent.get(&record.id).cloned();
                record
            })
            .collect())
    }

    pub async fn record_agent_event(&self, record: AgentTimelineEventRecord) -> Result<()> {
        use entities::agent_event;
        agent_event::ActiveModel {
            id: Set(record.event_id),
            session_id: Set(record.session_id),
            sequence: Set(record.sequence),
            kind: Set(record.kind),
            agent_id: Set(record.agent_id),
            path: Set(record.path),
            parent_path: Set(record.parent_path),
            payload_json: Set(record.payload_json),
            created_at: Set(record.created_at),
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    pub async fn list_agent_events(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentTimelineEventRecord>> {
        use entities::agent_event;
        let rows = agent_event::Entity::find()
            .filter(agent_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(agent_event::Column::Sequence)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(agent_timeline_event_record).collect())
    }

    pub async fn read_agent_event(
        &self,
        event_id: &str,
    ) -> Result<Option<AgentTimelineEventRecord>> {
        use entities::agent_event;
        Ok(agent_event::Entity::find_by_id(event_id.to_string())
            .one(&self.db)
            .await?
            .map(agent_timeline_event_record))
    }

    pub async fn next_agent_event_sequence(&self, session_id: &str) -> Result<i64> {
        use entities::agent_event;
        let max_seq = agent_event::Entity::find()
            .filter(agent_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_desc(agent_event::Column::Sequence)
            .one(&self.db)
            .await?
            .map(|row| row.sequence);
        Ok(max_seq.map(|sequence| sequence + 1).unwrap_or(0))
    }
}

pub(super) async fn upsert_agent_snapshot_with_tx(
    tx: &DatabaseTransaction,
    record: AgentSnapshotRecord,
) -> Result<()> {
    use entities::agent;
    if let Some(existing) = agent::Entity::find_by_id(record.id.clone()).one(tx).await? {
        let mut active: agent::ActiveModel = existing.into();
        active.session_id = Set(record.session_id);
        active.path = Set(record.path);
        active.parent_path = Set(record.parent_path);
        active.role = Set(record.role);
        active.task = Set(record.task);
        active.status = Set(record.status.as_str().to_string());
        active.summary = Set(record.summary);
        active.error = Set(record.error);
        active.reason = Set(record.reason);
        active.budget_limit_kind = Set(record
            .budget_limit_kind
            .map(|kind| kind.as_str().to_string()));
        active.budget_usage_json = Set(record
            .budget_usage
            .and_then(|usage| serde_json::to_string(&usage).ok()));
        active.depth = Set(record.depth);
        active.updated_at = Set(record.updated_at);
        active.update(tx).await?;
    } else {
        agent::ActiveModel {
            id: Set(record.id),
            session_id: Set(record.session_id),
            path: Set(record.path),
            parent_path: Set(record.parent_path),
            role: Set(record.role),
            task: Set(record.task),
            status: Set(record.status.as_str().to_string()),
            summary: Set(record.summary),
            error: Set(record.error),
            reason: Set(record.reason),
            budget_limit_kind: Set(record
                .budget_limit_kind
                .map(|kind| kind.as_str().to_string())),
            budget_usage_json: Set(record
                .budget_usage
                .and_then(|usage| serde_json::to_string(&usage).ok())),
            depth: Set(record.depth),
            updated_at: Set(record.updated_at),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

pub(super) async fn insert_agent_event_with_tx(
    tx: &DatabaseTransaction,
    record: AgentTimelineEventRecord,
) -> Result<()> {
    use entities::agent_event;
    agent_event::ActiveModel {
        id: Set(record.event_id),
        session_id: Set(record.session_id),
        sequence: Set(record.sequence),
        kind: Set(record.kind),
        agent_id: Set(record.agent_id),
        path: Set(record.path),
        parent_path: Set(record.parent_path),
        payload_json: Set(record.payload_json),
        created_at: Set(record.created_at),
    }
    .insert(tx)
    .await?;
    Ok(())
}

pub(super) fn agent_timeline_event_record_from_event(
    session_id: &str,
    event: &StudioAgentTimelineEvent,
) -> Option<AgentTimelineEventRecord> {
    let (kind, agent_id, path, parent_path) = match &event.kind {
        StudioAgentTimelineEventKind::SubAgentActivity {
            agent_id,
            path,
            parent_path,
            kind,
            ..
        } => (
            kind.as_str().to_string(),
            agent_id.clone(),
            path.clone(),
            parent_path.clone(),
        ),
    };
    Some(AgentTimelineEventRecord {
        event_id: event.event_id.clone(),
        session_id: session_id.to_string(),
        sequence: event.sequence as i64,
        kind,
        agent_id,
        path,
        parent_path,
        payload_json: serde_json::to_string(event).ok()?,
        created_at: event.created_at,
    })
}

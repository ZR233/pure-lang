use anyhow::Result;
use pl_protocol::SkillActivation;
use pl_trace::{TraceEvent, TraceEventKind};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};

use crate::studio::entities;
use crate::studio::mappers::session_skill_record;
use crate::studio::records::SessionSkillRecord;
use crate::studio::store::StudioStore;

impl StudioStore {
    pub async fn upsert_session_skill(
        &self,
        session_id: &str,
        activation: &SkillActivation,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        upsert_session_skill_with_tx(&tx, session_id, activation).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_session_skills(&self, session_id: &str) -> Result<Vec<SessionSkillRecord>> {
        use entities::session_skill;
        let rows = session_skill::Entity::find()
            .filter(session_skill::Column::SessionId.eq(session_id.to_string()))
            .order_by_desc(session_skill::Column::UpdatedAt)
            .order_by_asc(session_skill::Column::SkillNameKey)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(session_skill_record).collect())
    }

    pub async fn list_session_skill_names(&self, session_id: &str) -> Result<Vec<String>> {
        Ok(self
            .list_session_skills(session_id)
            .await?
            .into_iter()
            .map(|skill| skill.skill_name)
            .collect())
    }
}

pub(super) async fn upsert_session_skill_events_with_tx(
    tx: &DatabaseTransaction,
    trace_events: &[TraceEvent],
) -> Result<()> {
    for event in trace_events {
        if let TraceEventKind::SkillActivated { activation } = &event.kind {
            upsert_session_skill_with_tx(tx, &event.session_id, activation).await?;
        }
    }
    Ok(())
}

pub(super) async fn upsert_session_skill_with_tx(
    tx: &DatabaseTransaction,
    session_id: &str,
    activation: &SkillActivation,
) -> Result<()> {
    use entities::session_skill;
    let skill_name_key = activation.name.to_ascii_lowercase();
    let id = session_skill_id(session_id, &activation.name);
    let existing = session_skill::Entity::find()
        .filter(session_skill::Column::SessionId.eq(session_id.to_string()))
        .filter(session_skill::Column::SkillNameKey.eq(skill_name_key.clone()))
        .one(tx)
        .await?;
    if let Some(row) = existing {
        let mut active: session_skill::ActiveModel = row.into();
        active.skill_name = Set(activation.name.clone());
        active.source = Set(activation.source.clone());
        active.path = Set(activation.path.clone());
        active.last_turn_id = Set(activation.turn_id.clone());
        active.last_tool_call_id = Set(activation.tool_call_id.clone());
        active.updated_at = Set(activation.activated_at);
        active.update(tx).await?;
    } else {
        session_skill::ActiveModel {
            id: Set(id),
            session_id: Set(session_id.to_string()),
            skill_name: Set(activation.name.clone()),
            skill_name_key: Set(skill_name_key),
            source: Set(activation.source.clone()),
            path: Set(activation.path.clone()),
            first_turn_id: Set(activation.turn_id.clone()),
            last_turn_id: Set(activation.turn_id.clone()),
            last_tool_call_id: Set(activation.tool_call_id.clone()),
            activated_at: Set(activation.activated_at),
            updated_at: Set(activation.activated_at),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

fn session_skill_id(session_id: &str, skill_name: &str) -> String {
    format!("{session_id}:{}", skill_name.to_ascii_lowercase())
}

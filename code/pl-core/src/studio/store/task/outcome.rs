use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeRecord, AgentOutcomeStatus, CreateAgentOutcome, UpdateAgentOutcome,
};

impl StudioStore {
    pub(crate) async fn create_agent_outcome(
        &self,
        input: CreateAgentOutcome,
    ) -> Result<AgentOutcomeRecord> {
        let now = unix_seconds();
        agent_outcome_record(
            entities::agent_outcome::ActiveModel {
                id: Set(new_id("agent-outcome")),
                task_run_id: Set(input.task_run_id),
                work_unit_id: Set(input.work_unit_id),
                agent_id: Set(input.agent_id),
                owner_path: Set(input.owner_path),
                initiated_by: Set(input.initiated_by),
                requested_by_call_id: Set(input.requested_by_call_id),
                role: Set(input.role),
                status: Set(input.status.as_str().to_string()),
                attempt: Set(input.attempt as i32),
                summary: Set(None),
                error: Set(None),
                delivery_json: Set(None),
                review_json: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&self.db)
            .await?,
        )
    }

    pub(crate) async fn update_agent_outcome(
        &self,
        outcome_id: &str,
        update: UpdateAgentOutcome,
    ) -> Result<AgentOutcomeRecord> {
        let model = entities::agent_outcome::Entity::find_by_id(outcome_id.to_string())
            .one(&self.db)
            .await?
            .context("agent outcome not found")?;
        let mut active: entities::agent_outcome::ActiveModel = model.into();
        active.status = Set(update.status.as_str().to_string());
        active.summary = Set(update.summary);
        active.error = Set(update.error);
        active.delivery_json = Set(update
            .delivery
            .map(|value| serde_json::to_string(&value))
            .transpose()?);
        active.review_json = Set(update
            .review
            .map(|value| serde_json::to_string(&value))
            .transpose()?);
        active.updated_at = Set(unix_seconds());
        agent_outcome_record(active.update(&self.db).await?)
    }

    pub(crate) async fn read_agent_outcome_by_agent(
        &self,
        task_run_id: &str,
        agent_id: &str,
    ) -> Result<Option<AgentOutcomeRecord>> {
        entities::agent_outcome::Entity::find()
            .filter(entities::agent_outcome::Column::TaskRunId.eq(task_run_id.to_string()))
            .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
            .one(&self.db)
            .await?
            .map(agent_outcome_record)
            .transpose()
    }

    pub(crate) async fn list_agent_outcomes(
        &self,
        task_run_id: &str,
    ) -> Result<Vec<AgentOutcomeRecord>> {
        entities::agent_outcome::Entity::find()
            .filter(entities::agent_outcome::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::agent_outcome::Column::CreatedAt)
            .order_by_asc(entities::agent_outcome::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(agent_outcome_record)
            .collect()
    }
}

fn agent_outcome_record(model: entities::agent_outcome::Model) -> Result<AgentOutcomeRecord> {
    Ok(AgentOutcomeRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        work_unit_id: model.work_unit_id,
        agent_id: model.agent_id,
        owner_path: model.owner_path,
        initiated_by: model.initiated_by,
        requested_by_call_id: model.requested_by_call_id,
        role: model.role,
        status: AgentOutcomeStatus::from_str(&model.status)
            .with_context(|| format!("invalid agent outcome status: {}", model.status))?,
        attempt: model.attempt as u32,
        summary: model.summary,
        error: model.error,
        delivery: model
            .delivery_json
            .map(|json| serde_json::from_str(&json))
            .transpose()?,
        review: model
            .review_json
            .map(|json| serde_json::from_str(&json))
            .transpose()?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

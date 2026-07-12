use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
#[cfg(test)]
use crate::studio::task_coordinator::UpdateAgentOutcome;
use crate::studio::task_coordinator::{
    AgentOutcomeRecord, AgentOutcomeStatus, CreateAgentOutcome, TaskRunPhase,
};

impl StudioStore {
    pub(crate) async fn create_explorer_outcome(
        &self,
        session_id: &str,
        input: CreateAgentOutcome,
    ) -> Result<Option<AgentOutcomeRecord>> {
        if input.work_unit_id.is_some() {
            anyhow::bail!("explorer outcome must not reference a work unit");
        }
        let tx = self.db.begin().await?;
        let Some(run) = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
            .filter(entities::task_run::Column::Phase.is_not_in([
                TaskRunPhase::Completed.as_str(),
                TaskRunPhase::Blocked.as_str(),
                TaskRunPhase::Failed.as_str(),
                TaskRunPhase::Cancelled.as_str(),
            ]))
            .order_by_desc(entities::task_run::Column::UpdatedAt)
            .order_by_desc(entities::task_run::Column::Id)
            .one(&tx)
            .await?
        else {
            tx.rollback().await?;
            return Ok(None);
        };
        if run.id != input.task_run_id {
            tx.rollback().await?;
            anyhow::bail!("explorer outcome task run does not match active session task");
        }
        let now = unix_seconds();
        let outcome = agent_outcome_record(
            entities::agent_outcome::ActiveModel {
                id: Set(new_id("agent-outcome")),
                task_run_id: Set(input.task_run_id),
                work_unit_id: Set(None),
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
                terminal_observed: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?,
        )?;
        tx.commit().await?;
        Ok(Some(outcome))
    }

    pub(crate) async fn update_spawned_outcome(
        &self,
        outcome_id: &str,
        agent_id: &str,
        status: AgentOutcomeStatus,
        error: Option<String>,
    ) -> Result<()> {
        let outcome = entities::agent_outcome::Entity::find_by_id(outcome_id.to_string())
            .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
            .one(&self.db)
            .await?
            .context("spawned agent outcome not found")?;
        let mut active: entities::agent_outcome::ActiveModel = outcome.into();
        active.status = Set(status.as_str().to_string());
        active.error = Set(error);
        active.updated_at = Set(unix_seconds());
        active.update(&self.db).await?;
        Ok(())
    }

    #[cfg(test)]
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
                terminal_observed: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&self.db)
            .await?,
        )
    }

    #[cfg(test)]
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

pub(super) fn agent_outcome_record(
    model: entities::agent_outcome::Model,
) -> Result<AgentOutcomeRecord> {
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

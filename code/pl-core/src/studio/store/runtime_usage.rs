use std::collections::HashMap;

use anyhow::Result;
use pl_protocol::{
    AgentRuntimeDelta, RuntimeUsageSnapshot, StudioRuntimeUsage, StudioSessionRuntime,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, EntityTrait, QueryFilter,
    TransactionTrait,
};

use crate::TurnResult;
use crate::runtime_usage::{
    ROOT_AGENT_ID, ROOT_AGENT_PATH, ROOT_AGENT_ROLE, aggregate_runtime_usage, cost_for_usage,
    merge_costs, token_usage_snapshot,
};
use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::{
    agent_runtime_snapshot_record, costs_to_json, session_runtime_record,
};
use crate::studio::records::SessionRuntimeRecord;
use crate::studio::store::StudioStore;

impl StudioStore {
    pub async fn agent_runtime_usage_by_agent(
        &self,
        session_id: &str,
    ) -> Result<HashMap<String, RuntimeUsageSnapshot>> {
        use entities::agent_runtime_snapshot;
        let rows = agent_runtime_snapshot::Entity::find()
            .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let agent_id = row.agent_id.clone();
                (agent_id, agent_runtime_snapshot_record(row))
            })
            .collect())
    }

    pub async fn record_agent_runtime_delta(
        &self,
        session_id: &str,
        delta: &AgentRuntimeDelta,
    ) -> Result<bool> {
        let tx = self.db.begin().await?;
        let updated = record_agent_runtime_delta_with_tx(&tx, session_id, delta).await?;
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn rebuild_session_runtime_from_agent_snapshots(
        &self,
        session_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        rebuild_session_runtime_from_agent_snapshots_with_tx(&tx, session_id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_session_runtime(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionRuntimeRecord>> {
        use entities::session_runtime_snapshot;
        Ok(
            session_runtime_snapshot::Entity::find_by_id(session_id.to_string())
                .one(&self.db)
                .await?
                .map(session_runtime_record),
        )
    }

    pub async fn upsert_session_runtime(
        &self,
        session_id: &str,
        result: &TurnResult,
        model: Option<&pl_model::ModelInfo>,
    ) -> Result<()> {
        let usage = token_usage_snapshot(&result.usage);
        if usage.total_tokens == 0 {
            return self
                .rebuild_session_runtime_from_agent_snapshots(session_id)
                .await;
        }
        let model_name = if result.model.is_empty() {
            model
                .map(|model| model.slug.clone())
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            result.model.clone()
        };
        let (estimated_costs, has_unpriced_usage) = cost_for_usage(&usage, model);
        let delta = AgentRuntimeDelta {
            inference_id: format!("root-usage-{}", result.session_message_count),
            agent_id: ROOT_AGENT_ID.to_string(),
            path: ROOT_AGENT_PATH.to_string(),
            parent_path: None,
            role: ROOT_AGENT_ROLE.to_string(),
            model: model_name,
            context_window: model.and_then(pl_model::ModelInfo::resolved_context_window),
            usage,
            estimated_costs,
            has_unpriced_usage,
            updated_at: unix_seconds(),
        };
        self.record_agent_runtime_delta(session_id, &delta).await?;
        Ok(())
    }

    pub async fn upsert_session_runtime_for_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        result: &TurnResult,
        model: Option<&pl_model::ModelInfo>,
    ) -> Result<()> {
        if self
            .has_agent_runtime_events_for_turn(session_id, turn_id)
            .await?
        {
            return self
                .rebuild_session_runtime_from_agent_snapshots(session_id)
                .await;
        }
        self.upsert_session_runtime(session_id, result, model).await
    }

    pub async fn has_agent_runtime_events_for_turn(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<bool> {
        use entities::agent_runtime_event;
        let inference_prefix = format!("{turn_id}-inf-");
        let compact_prefix = format!("{turn_id}-compact-");
        Ok(agent_runtime_event::Entity::find()
            .filter(agent_runtime_event::Column::SessionId.eq(session_id.to_string()))
            .filter(
                Condition::any()
                    .add(agent_runtime_event::Column::InferenceId.starts_with(inference_prefix))
                    .add(agent_runtime_event::Column::InferenceId.starts_with(compact_prefix)),
            )
            .one(&self.db)
            .await?
            .is_some())
    }
}

pub(super) fn runtime_usage_snapshot(usage: StudioRuntimeUsage) -> RuntimeUsageSnapshot {
    RuntimeUsageSnapshot {
        model: usage.model,
        context_window: usage.context_window,
        latest_context_tokens: usage.latest_context_tokens,
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_prompt_tokens: usage.cached_prompt_tokens,
        total_tokens: usage.total_tokens,
        estimated_costs: usage.estimated_costs,
        has_unpriced_usage: usage.has_unpriced_usage,
        updated_at: usage.updated_at,
    }
}

pub(super) async fn upsert_session_runtime_snapshot_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
    runtime: &StudioSessionRuntime,
) -> Result<()> {
    use entities::session_runtime_snapshot;
    let usage = runtime_usage_snapshot(runtime.usage.clone());
    let (currency, estimated_cost) = if usage.estimated_costs.len() == 1 {
        (
            Some(usage.estimated_costs[0].currency.clone()),
            Some(usage.estimated_costs[0].amount),
        )
    } else {
        (None, None)
    };
    let costs_json = costs_to_json(&usage.estimated_costs);
    if let Some(row) = session_runtime_snapshot::Entity::find_by_id(session_id.to_string())
        .one(tx)
        .await?
    {
        if row.updated_at > usage.updated_at {
            return Ok(());
        }
        let mut active: session_runtime_snapshot::ActiveModel = row.into();
        active.model = Set(usage.model);
        active.context_window = Set(usage.context_window.map(|value| value as i64));
        active.latest_context_tokens = Set(usage.latest_context_tokens as i64);
        active.prompt_tokens = Set(usage.prompt_tokens as i64);
        active.completion_tokens = Set(usage.completion_tokens as i64);
        active.cached_prompt_tokens = Set(usage.cached_prompt_tokens as i64);
        active.total_tokens = Set(usage.total_tokens as i64);
        active.currency = Set(currency);
        active.estimated_cost = Set(estimated_cost);
        active.estimated_costs_json = Set(costs_json);
        active.has_unpriced_usage = Set(i32::from(usage.has_unpriced_usage));
        active.updated_at = Set(usage.updated_at);
        active.update(tx).await?;
    } else {
        session_runtime_snapshot::ActiveModel {
            session_id: Set(session_id.to_string()),
            model: Set(usage.model),
            context_window: Set(usage.context_window.map(|value| value as i64)),
            latest_context_tokens: Set(usage.latest_context_tokens as i64),
            prompt_tokens: Set(usage.prompt_tokens as i64),
            completion_tokens: Set(usage.completion_tokens as i64),
            cached_prompt_tokens: Set(usage.cached_prompt_tokens as i64),
            total_tokens: Set(usage.total_tokens as i64),
            currency: Set(currency),
            estimated_cost: Set(estimated_cost),
            estimated_costs_json: Set(costs_json),
            has_unpriced_usage: Set(i32::from(usage.has_unpriced_usage)),
            updated_at: Set(usage.updated_at),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

async fn record_agent_runtime_delta_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
    delta: &AgentRuntimeDelta,
) -> Result<bool> {
    use entities::{agent_runtime_event, agent_runtime_snapshot};

    let exists = agent_runtime_event::Entity::find()
        .filter(agent_runtime_event::Column::SessionId.eq(session_id.to_string()))
        .filter(agent_runtime_event::Column::InferenceId.eq(delta.inference_id.clone()))
        .one(tx)
        .await?
        .is_some();
    if exists {
        return Ok(false);
    }

    let costs_json = costs_to_json(&delta.estimated_costs);
    agent_runtime_event::ActiveModel {
        id: Set(new_id("agent-runtime-event")),
        session_id: Set(session_id.to_string()),
        inference_id: Set(delta.inference_id.clone()),
        agent_id: Set(delta.agent_id.clone()),
        path: Set(delta.path.clone()),
        parent_path: Set(delta.parent_path.clone()),
        role: Set(delta.role.clone()),
        model: Set(delta.model.clone()),
        context_window: Set(delta.context_window.map(|value| value as i64)),
        prompt_tokens: Set(delta.usage.prompt_tokens as i64),
        completion_tokens: Set(delta.usage.completion_tokens as i64),
        cached_prompt_tokens: Set(delta.usage.cached_prompt_tokens as i64),
        total_tokens: Set(delta.usage.total_tokens as i64),
        estimated_costs_json: Set(costs_json.clone()),
        has_unpriced_usage: Set(i32::from(delta.has_unpriced_usage)),
        created_at: Set(delta.updated_at),
    }
    .insert(tx)
    .await?;

    let existing = agent_runtime_snapshot::Entity::find()
        .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
        .filter(agent_runtime_snapshot::Column::AgentId.eq(delta.agent_id.clone()))
        .one(tx)
        .await?;
    if let Some(row) = existing {
        let mut costs = crate::studio::mappers::costs_from_json(&row.estimated_costs_json);
        merge_costs(&mut costs, &delta.estimated_costs);
        let prompt_tokens = row.prompt_tokens + delta.usage.prompt_tokens as i64;
        let completion_tokens = row.completion_tokens + delta.usage.completion_tokens as i64;
        let cached_prompt_tokens =
            row.cached_prompt_tokens + delta.usage.cached_prompt_tokens as i64;
        let total_tokens = row.total_tokens + delta.usage.total_tokens as i64;
        let has_unpriced_usage = row.has_unpriced_usage != 0 || delta.has_unpriced_usage;
        let mut active: agent_runtime_snapshot::ActiveModel = row.into();
        active.path = Set(delta.path.clone());
        active.parent_path = Set(delta.parent_path.clone());
        active.role = Set(delta.role.clone());
        active.model = Set(delta.model.clone());
        active.context_window = Set(delta.context_window.map(|value| value as i64));
        active.latest_context_tokens = Set(delta.usage.prompt_tokens as i64);
        active.prompt_tokens = Set(prompt_tokens);
        active.completion_tokens = Set(completion_tokens);
        active.cached_prompt_tokens = Set(cached_prompt_tokens);
        active.total_tokens = Set(total_tokens);
        active.estimated_costs_json = Set(costs_to_json(&costs));
        active.has_unpriced_usage = Set(i32::from(has_unpriced_usage));
        active.updated_at = Set(delta.updated_at);
        active.update(tx).await?;
    } else {
        agent_runtime_snapshot::ActiveModel {
            id: Set(runtime_snapshot_id(session_id, &delta.agent_id)),
            session_id: Set(session_id.to_string()),
            agent_id: Set(delta.agent_id.clone()),
            path: Set(delta.path.clone()),
            parent_path: Set(delta.parent_path.clone()),
            role: Set(delta.role.clone()),
            model: Set(delta.model.clone()),
            context_window: Set(delta.context_window.map(|value| value as i64)),
            latest_context_tokens: Set(delta.usage.prompt_tokens as i64),
            prompt_tokens: Set(delta.usage.prompt_tokens as i64),
            completion_tokens: Set(delta.usage.completion_tokens as i64),
            cached_prompt_tokens: Set(delta.usage.cached_prompt_tokens as i64),
            total_tokens: Set(delta.usage.total_tokens as i64),
            estimated_costs_json: Set(costs_json),
            has_unpriced_usage: Set(i32::from(delta.has_unpriced_usage)),
            updated_at: Set(delta.updated_at),
        }
        .insert(tx)
        .await?;
    }

    rebuild_session_runtime_from_agent_snapshots_with_tx(tx, session_id).await?;
    Ok(true)
}

async fn rebuild_session_runtime_from_agent_snapshots_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
) -> Result<()> {
    use entities::{agent_runtime_snapshot, session_runtime_snapshot};

    let rows = agent_runtime_snapshot::Entity::find()
        .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
        .all(tx)
        .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let aggregate = aggregate_runtime_usage(
        "unknown",
        rows.into_iter().map(agent_runtime_snapshot_record),
    );
    let (currency, estimated_cost) = if aggregate.estimated_costs.len() == 1 {
        (
            Some(aggregate.estimated_costs[0].currency.clone()),
            Some(aggregate.estimated_costs[0].amount),
        )
    } else {
        (None, None)
    };
    let costs_json = costs_to_json(&aggregate.estimated_costs);

    if let Some(row) = session_runtime_snapshot::Entity::find_by_id(session_id.to_string())
        .one(tx)
        .await?
    {
        let mut active: session_runtime_snapshot::ActiveModel = row.into();
        active.model = Set(aggregate.model);
        active.context_window = Set(aggregate.context_window.map(|value| value as i64));
        active.latest_context_tokens = Set(aggregate.latest_context_tokens as i64);
        active.prompt_tokens = Set(aggregate.prompt_tokens as i64);
        active.completion_tokens = Set(aggregate.completion_tokens as i64);
        active.cached_prompt_tokens = Set(aggregate.cached_prompt_tokens as i64);
        active.total_tokens = Set(aggregate.total_tokens as i64);
        active.currency = Set(currency);
        active.estimated_cost = Set(estimated_cost);
        active.estimated_costs_json = Set(costs_json);
        active.has_unpriced_usage = Set(i32::from(aggregate.has_unpriced_usage));
        active.updated_at = Set(aggregate.updated_at);
        active.update(tx).await?;
    } else {
        session_runtime_snapshot::ActiveModel {
            session_id: Set(session_id.to_string()),
            model: Set(aggregate.model),
            context_window: Set(aggregate.context_window.map(|value| value as i64)),
            latest_context_tokens: Set(aggregate.latest_context_tokens as i64),
            prompt_tokens: Set(aggregate.prompt_tokens as i64),
            completion_tokens: Set(aggregate.completion_tokens as i64),
            cached_prompt_tokens: Set(aggregate.cached_prompt_tokens as i64),
            total_tokens: Set(aggregate.total_tokens as i64),
            currency: Set(currency),
            estimated_cost: Set(estimated_cost),
            estimated_costs_json: Set(costs_json),
            has_unpriced_usage: Set(i32::from(aggregate.has_unpriced_usage)),
            updated_at: Set(aggregate.updated_at),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

fn runtime_snapshot_id(session_id: &str, agent_id: &str) -> String {
    format!("{session_id}:{agent_id}")
}

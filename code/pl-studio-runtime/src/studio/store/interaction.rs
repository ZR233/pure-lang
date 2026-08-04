use std::collections::BTreeSet;

use crate::{InteractionKind, InteractionRequest, InteractionStatus};
use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::mappers::interaction_record;
use crate::studio::store::StudioStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestartUserInputRecoveryReceipt {
    interaction_id: String,
    recovered_at: i64,
}

impl StudioStore {
    pub async fn upsert_interaction(&self, interaction: &InteractionRequest) -> Result<()> {
        use entities::interaction;
        let payload_json = serde_json::to_string(&interaction.payload)?;
        let resolution_json = interaction
            .resolution
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        if let Some(existing) = interaction::Entity::find_by_id(interaction.interaction_id.clone())
            .one(&self.db)
            .await?
        {
            let mut active: interaction::ActiveModel = existing.into();
            active.thread_id = Set(interaction.scope.thread_id.clone());
            active.turn_id = Set(interaction.scope.turn_id.clone());
            active.item_id = Set(interaction.scope.item_id.clone());
            active.tool_id = Set(interaction.scope.tool_id.clone());
            active.agent_path = Set(interaction.scope.agent_path.clone());
            active.kind = Set(interaction.kind.as_str().to_string());
            active.status = Set(interaction.status.as_str().to_string());
            active.payload_json = Set(payload_json);
            active.resolution_json = Set(resolution_json);
            active.updated_at = Set(interaction.updated_at);
            active.resolved_at = Set(interaction.resolved_at);
            active.update(&self.db).await?;
        } else {
            interaction::ActiveModel {
                id: Set(interaction.interaction_id.clone()),
                thread_id: Set(interaction.scope.thread_id.clone()),
                turn_id: Set(interaction.scope.turn_id.clone()),
                item_id: Set(interaction.scope.item_id.clone()),
                tool_id: Set(interaction.scope.tool_id.clone()),
                agent_path: Set(interaction.scope.agent_path.clone()),
                kind: Set(interaction.kind.as_str().to_string()),
                status: Set(interaction.status.as_str().to_string()),
                payload_json: Set(payload_json),
                resolution_json: Set(resolution_json),
                created_at: Set(interaction.created_at),
                updated_at: Set(interaction.updated_at),
                resolved_at: Set(interaction.resolved_at),
            }
            .insert(&self.db)
            .await?;
        }
        Ok(())
    }

    pub async fn read_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<Option<InteractionRequest>> {
        use entities::interaction;
        interaction::Entity::find_by_id(interaction_id.to_string())
            .one(&self.db)
            .await?
            .map(interaction_record)
            .transpose()
    }

    pub async fn list_pending_interactions(
        &self,
        thread_id: &str,
    ) -> Result<Vec<InteractionRequest>> {
        use entities::interaction;
        interaction::Entity::find()
            .filter(interaction::Column::ThreadId.eq(thread_id.to_string()))
            .filter(interaction::Column::Status.eq("pending"))
            .order_by_desc(interaction::Column::UpdatedAt)
            .order_by_desc(interaction::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(interaction_record)
            .collect()
    }

    pub async fn list_threads_with_transient_pending_interactions(&self) -> Result<Vec<String>> {
        use entities::interaction;
        let rows = interaction::Entity::find()
            .filter(interaction::Column::Status.eq(InteractionStatus::Pending.as_str()))
            .filter(interaction::Column::Kind.is_in([
                InteractionKind::UserInput.as_str(),
                InteractionKind::ToolApproval.as_str(),
                InteractionKind::PlanConfirmation.as_str(),
            ]))
            .all(&self.db)
            .await?;
        let mut thread_ids = rows
            .into_iter()
            .map(|row| row.thread_id)
            .collect::<Vec<_>>();
        thread_ids.sort();
        thread_ids.dedup();
        Ok(thread_ids)
    }

    pub(in crate::studio) async fn list_restart_recoverable_user_inputs(
        &self,
    ) -> Result<Vec<InteractionRequest>> {
        use entities::{interaction, thread, turn};
        let active_threads = thread::Entity::find()
            .filter(thread::Column::Archived.eq(0))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|thread| thread.id)
            .collect::<BTreeSet<_>>();
        let pending_threads = interaction::Entity::find()
            .filter(interaction::Column::Kind.eq(InteractionKind::UserInput.as_str()))
            .filter(interaction::Column::Status.eq(InteractionStatus::Pending.as_str()))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|interaction| interaction.thread_id)
            .collect::<BTreeSet<_>>();
        let restarted_turns = turn::Entity::find()
            .filter(turn::Column::Status.eq("interrupted"))
            .filter(turn::Column::Reason.eq("runtime_restarted"))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|turn| (turn.thread_id, turn.id))
            .collect::<BTreeSet<_>>();
        let rows = interaction::Entity::find()
            .filter(interaction::Column::Kind.eq(InteractionKind::UserInput.as_str()))
            .filter(interaction::Column::Status.eq(InteractionStatus::Cancelled.as_str()))
            .order_by_asc(interaction::Column::ThreadId)
            .order_by_desc(interaction::Column::UpdatedAt)
            .order_by_desc(interaction::Column::Id)
            .all(&self.db)
            .await?;
        let mut seen_threads = BTreeSet::new();
        let mut recoverable = Vec::new();
        for row in rows {
            if !active_threads.contains(&row.thread_id)
                || pending_threads.contains(&row.thread_id)
                || !restarted_turns.contains(&(row.thread_id.clone(), row.turn_id.clone()))
                || !seen_threads.insert(row.thread_id.clone())
            {
                continue;
            }
            let interaction = interaction_record(row)?;
            if self.restart_user_input_was_recovered(&interaction).await? {
                continue;
            }
            recoverable.push(interaction);
        }
        Ok(recoverable)
    }

    pub(in crate::studio) async fn mark_restart_user_input_recovered(
        &self,
        interaction: &InteractionRequest,
    ) -> Result<()> {
        use entities::turn;
        let tx = self.db.begin().await?;
        let row = turn::Entity::find_by_id(interaction.scope.turn_id.clone())
            .filter(turn::Column::ThreadId.eq(interaction.scope.thread_id.clone()))
            .one(&tx)
            .await?;
        let receipt = serde_json::to_value(RestartUserInputRecoveryReceipt {
            interaction_id: interaction.interaction_id.clone(),
            recovered_at: unix_seconds(),
        })?;
        if let Some(row) = row {
            let mut metadata = row
                .metadata_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?
                .unwrap_or_else(|| serde_json::json!({}));
            let metadata = metadata
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("agent turn metadata must be a JSON object"))?;
            metadata.insert("recoveredInteraction".to_string(), receipt.clone());
            let mut active = row.into_active_model();
            active.metadata_json = Set(Some(serde_json::to_string(metadata)?));
            active.update(&tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn restart_user_input_was_recovered(
        &self,
        interaction: &InteractionRequest,
    ) -> Result<bool> {
        use entities::turn;
        let row = turn::Entity::find_by_id(interaction.scope.turn_id.clone())
            .filter(turn::Column::ThreadId.eq(interaction.scope.thread_id.clone()))
            .one(&self.db)
            .await?;
        if let Some(row) = row {
            let receipt = row
                .metadata_json
                .as_deref()
                .map(serde_json::from_str::<serde_json::Value>)
                .transpose()?
                .and_then(|metadata| metadata.get("recoveredInteraction").cloned())
                .map(serde_json::from_value::<RestartUserInputRecoveryReceipt>)
                .transpose()?;
            if receipt
                .as_ref()
                .is_some_and(|receipt| receipt.interaction_id == interaction.interaction_id)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        InteractionPayload, InteractionResolution, InteractionScope, StudioMode, UserInputAnswer,
    };

    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    use super::*;

    async fn store_with_session(path: &str) -> (StudioStore, String) {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project(path).await.unwrap();
        let session = store
            .create_thread(&project.id, "Restart interaction", StudioMode::Task)
            .await
            .unwrap();
        (store, session.id)
    }

    fn cancelled_user_input(
        interaction_id: &str,
        session_id: &str,
        turn_id: &str,
        updated_at: i64,
    ) -> InteractionRequest {
        InteractionRequest {
            interaction_id: interaction_id.to_string(),
            kind: InteractionKind::UserInput,
            status: InteractionStatus::Cancelled,
            scope: InteractionScope {
                thread_id: session_id.to_string(),
                turn_id: turn_id.to_string(),
                item_id: Some(interaction_id.to_string()),
                tool_id: Some(interaction_id.to_string()),
                agent_path: None,
            },
            payload: InteractionPayload::UserInput {
                questions: Vec::new(),
            },
            created_at: updated_at,
            updated_at,
            resolved_at: Some(updated_at),
            resolution: Some(InteractionResolution::UserInput {
                answers: std::collections::HashMap::<String, UserInputAnswer>::new(),
            }),
        }
    }

    async fn insert_cancelled_turn(
        store: &StudioStore,
        session_id: &str,
        turn_id: &str,
        metadata: Option<serde_json::Value>,
    ) {
        store
            .database()
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO turns (
                     id, thread_id, ordinal, revision, status, phase, reason,
                     model_json, usage_json, failure_json, metadata_json,
                     started_at, updated_at, completed_at
                 ) VALUES (?, ?, 1, 0, 'interrupted', NULL, 'runtime_restarted',
                           NULL, '{}', NULL, ?, 1, 2, 2)",
                [
                    turn_id.to_string().into(),
                    session_id.to_string().into(),
                    metadata.map(|value| value.to_string()).into(),
                ],
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn restart_recovery_receipt_on_thread_prevents_duplicate_recovery() {
        let (store, session_id) = store_with_session("C:/work/restart-receipt").await;
        let interaction = cancelled_user_input("ask-receipted", &session_id, "turn-receipted", 1);
        store.upsert_interaction(&interaction).await.unwrap();
        insert_cancelled_turn(
            &store,
            &session_id,
            "turn-receipted",
            Some(serde_json::json!({
                "recoveredInteraction": {
                    "interactionId": interaction.interaction_id,
                    "recoveredAt": 10
                }
            })),
        )
        .await;

        assert!(
            store
                .list_restart_recoverable_user_inputs()
                .await
                .unwrap()
                .is_empty()
        );
    }
}

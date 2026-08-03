use std::collections::BTreeSet;

use crate::{InteractionKind, InteractionRequest, InteractionStatus};
use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait,
    QueryFilter, QueryOrder, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::studio::entities;
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
            active.session_id = Set(interaction.scope.session_id.clone());
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
                session_id: Set(interaction.scope.session_id.clone()),
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
        session_id: &str,
    ) -> Result<Vec<InteractionRequest>> {
        use entities::interaction;
        interaction::Entity::find()
            .filter(interaction::Column::SessionId.eq(session_id.to_string()))
            .filter(interaction::Column::Status.eq("pending"))
            .order_by_desc(interaction::Column::UpdatedAt)
            .order_by_desc(interaction::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(interaction_record)
            .collect()
    }

    pub async fn list_sessions_with_transient_pending_interactions(&self) -> Result<Vec<String>> {
        use entities::interaction;
        let rows = interaction::Entity::find()
            .filter(interaction::Column::Status.eq(InteractionStatus::Pending.as_str()))
            .filter(interaction::Column::Kind.is_in([
                InteractionKind::UserInput.as_str(),
                InteractionKind::ToolApproval.as_str(),
            ]))
            .all(&self.db)
            .await?;
        let mut session_ids = rows
            .into_iter()
            .map(|row| row.session_id)
            .collect::<Vec<_>>();
        session_ids.sort();
        session_ids.dedup();
        Ok(session_ids)
    }

    pub(in crate::studio) async fn list_restart_recoverable_user_inputs(
        &self,
    ) -> Result<Vec<InteractionRequest>> {
        let rows = self
            .db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT interaction.id AS interaction_id,
                        interaction.session_id AS session_id
                 FROM interactions interaction
                 INNER JOIN sessions session ON session.id = interaction.session_id
                 WHERE interaction.kind = 'userInput'
                   AND interaction.status = 'cancelled'
                   AND session.archived = 0
                   AND session.visibility = 'active'
                   AND EXISTS (
                       SELECT 1 FROM agent_turns turn
                       WHERE turn.session_id = interaction.session_id
                         AND turn.turn_id = interaction.turn_id
                         AND turn.status = 'cancelled'
                         AND turn.reason = 'runtime_restarted'
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM interactions pending
                       WHERE pending.session_id = interaction.session_id
                         AND pending.kind = 'userInput'
                         AND pending.status = 'pending'
                   )
                 ORDER BY interaction.session_id,
                          interaction.updated_at DESC,
                          interaction.id DESC"
                    .to_string(),
            ))
            .await?;
        let mut seen_sessions = BTreeSet::new();
        let mut recoverable = Vec::new();
        for row in rows {
            let session_id: String = row.try_get("", "session_id")?;
            if !seen_sessions.insert(session_id) {
                continue;
            }
            let interaction_id: String = row.try_get("", "interaction_id")?;
            if let Some(interaction) = self.read_interaction(&interaction_id).await? {
                if self.restart_user_input_was_recovered(&interaction).await? {
                    continue;
                }
                recoverable.push(interaction);
            }
        }
        Ok(recoverable)
    }

    pub(in crate::studio) async fn mark_restart_user_input_recovered(
        &self,
        interaction: &InteractionRequest,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let rows = tx
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT agent_id, metadata_json
                 FROM agent_turns
                 WHERE session_id = ? AND turn_id = ?
                 ORDER BY agent_id",
                [
                    interaction.scope.session_id.clone().into(),
                    interaction.scope.turn_id.clone().into(),
                ],
            ))
            .await?;
        let receipt = serde_json::to_value(RestartUserInputRecoveryReceipt {
            interaction_id: interaction.interaction_id.clone(),
            recovered_at: unix_seconds(),
        })?;
        for row in rows {
            let agent_id: String = row.try_get("", "agent_id")?;
            let metadata_json: Option<String> = row.try_get("", "metadata_json")?;
            let mut metadata = metadata_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?
                .unwrap_or_else(|| serde_json::json!({}));
            let metadata = metadata
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("agent turn metadata must be a JSON object"))?;
            metadata.insert("recoveredInteraction".to_string(), receipt.clone());
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE agent_turns
                     SET metadata_json = ?
                     WHERE agent_id = ? AND turn_id = ?",
                [
                    serde_json::to_string(metadata)?.into(),
                    agent_id.into(),
                    interaction.scope.turn_id.clone().into(),
                ],
            ))
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn restart_user_input_was_recovered(
        &self,
        interaction: &InteractionRequest,
    ) -> Result<bool> {
        let rows = self
            .db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT metadata_json
                 FROM agent_turns
                 WHERE session_id = ? AND turn_id = ?
                 ORDER BY agent_id",
                [
                    interaction.scope.session_id.clone().into(),
                    interaction.scope.turn_id.clone().into(),
                ],
            ))
            .await?;
        for row in rows {
            let metadata_json: Option<String> = row.try_get("", "metadata_json")?;
            let receipt = metadata_json
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
    use pretty_assertions::assert_eq;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    use super::*;

    async fn store_with_session(path: &str) -> (StudioStore, String) {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project(path).await.unwrap();
        let session = store
            .create_session(&project.id, "Restart interaction", StudioMode::Task)
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
                session_id: session_id.to_string(),
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
        agent_id: &str,
        session_id: &str,
        turn_id: &str,
        metadata: Option<serde_json::Value>,
    ) {
        store
            .database()
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO agent_turns (
                     agent_id, turn_id, session_id, status, reason, usage_json,
                     metadata_json, started_at, finished_at
                 ) VALUES (?, ?, ?, 'cancelled', 'runtime_restarted', '{}', ?, 1, 2)",
                [
                    agent_id.to_string().into(),
                    turn_id.to_string().into(),
                    session_id.to_string().into(),
                    metadata.map(|value| value.to_string()).into(),
                ],
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn restart_recovery_selects_only_latest_user_input_per_session() {
        let (store, session_id) = store_with_session("C:/work/restart-latest").await;
        let older = cancelled_user_input("ask-older", &session_id, "turn-older", 1);
        let latest = cancelled_user_input("ask-latest", &session_id, "turn-latest", 2);
        store.upsert_interaction(&older).await.unwrap();
        store.upsert_interaction(&latest).await.unwrap();
        insert_cancelled_turn(&store, "agent-root", &session_id, "turn-older", None).await;
        insert_cancelled_turn(&store, "agent-root", &session_id, "turn-latest", None).await;

        assert_eq!(
            store.list_restart_recoverable_user_inputs().await.unwrap(),
            vec![latest.clone()]
        );

        let mut pending = latest;
        pending.interaction_id = "ask-new-pending".to_string();
        pending.status = InteractionStatus::Pending;
        pending.resolved_at = None;
        pending.resolution = None;
        store.upsert_interaction(&pending).await.unwrap();
        assert!(
            store
                .list_restart_recoverable_user_inputs()
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn restart_recovery_receipt_on_any_owner_prevents_recovery() {
        let (store, session_id) = store_with_session("C:/work/restart-owner").await;
        let interaction = cancelled_user_input("ask-receipted", &session_id, "turn-receipted", 1);
        store.upsert_interaction(&interaction).await.unwrap();
        insert_cancelled_turn(
            &store,
            "agent-a",
            &session_id,
            "turn-receipted",
            Some(serde_json::json!({"owner": "a"})),
        )
        .await;
        insert_cancelled_turn(
            &store,
            "agent-b",
            &session_id,
            "turn-receipted",
            Some(serde_json::json!({
                "owner": "b",
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

    #[tokio::test]
    async fn marking_restart_recovery_is_idempotent_and_preserves_turn_metadata() {
        let (store, session_id) = store_with_session("C:/work/restart-mark").await;
        let interaction = cancelled_user_input("ask-mark", &session_id, "turn-mark", 1);
        store.upsert_interaction(&interaction).await.unwrap();
        for agent_id in ["agent-a", "agent-b"] {
            insert_cancelled_turn(
                &store,
                agent_id,
                &session_id,
                "turn-mark",
                Some(serde_json::json!({"owner": agent_id})),
            )
            .await;
        }

        store
            .mark_restart_user_input_recovered(&interaction)
            .await
            .unwrap();
        store
            .mark_restart_user_input_recovered(&interaction)
            .await
            .unwrap();

        assert!(
            store
                .list_restart_recoverable_user_inputs()
                .await
                .unwrap()
                .is_empty()
        );
        let rows = store
            .database()
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT agent_id, metadata_json
                 FROM agent_turns
                 WHERE session_id = ? AND turn_id = ?
                 ORDER BY agent_id",
                [session_id.into(), "turn-mark".into()],
            ))
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        for row in rows {
            let agent_id: String = row.try_get("", "agent_id").unwrap();
            let metadata_json: String = row.try_get("", "metadata_json").unwrap();
            let metadata: serde_json::Value = serde_json::from_str(&metadata_json).unwrap();
            assert_eq!(metadata["owner"], agent_id);
            assert_eq!(
                metadata["recoveredInteraction"]["interactionId"],
                interaction.interaction_id
            );
        }
    }
}

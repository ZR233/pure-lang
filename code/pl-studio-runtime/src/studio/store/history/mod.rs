use anyhow::Result;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};

use crate::studio::entity::{
    history_gc_job, session_history_checkpoint, session_history_item, session_history_turn,
};
use crate::studio::store::StudioStore;

pub(crate) mod persistence;
mod query;

impl StudioStore {
    pub(super) fn spawn_history_gc(&self) {
        let store = self.clone();
        tokio::spawn(async move {
            if let Err(error) = store.run_history_gc_jobs().await {
                tracing::warn!(
                    error_bytes = error.to_string().len(),
                    "history garbage collection deferred"
                );
            }
        });
    }

    async fn run_history_gc_jobs(&self) -> Result<()> {
        let jobs = history_gc_job::Entity::find().all(&self.db).await?;
        for job in jobs {
            self.delete_session_history(&job.session_id).await?;
            history_gc_job::Entity::delete_by_id(job.id)
                .exec(&self.db)
                .await?;
            tracing::trace!(session_id = %job.session_id, "deleted orphaned session history");
        }
        Ok(())
    }

    async fn delete_session_history(&self, session_id: &str) -> Result<()> {
        let transaction = self.history_db.begin().await?;
        session_history_item::Entity::delete_many()
            .filter(session_history_item::Column::SessionId.eq(session_id))
            .exec(&transaction)
            .await?;
        session_history_checkpoint::Entity::delete_many()
            .filter(session_history_checkpoint::Column::SessionId.eq(session_id))
            .exec(&transaction)
            .await?;
        session_history_turn::Entity::delete_many()
            .filter(session_history_turn::Column::SessionId.eq(session_id))
            .exec(&transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{
        ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseBackend, EntityTrait,
        QueryFilter, Statement,
    };

    use super::*;
    use crate::studio::entity::{history_gc_job, session_history_checkpoint};
    use pl_protocol::{
        SessionEventEnvelope, SessionEventKind, SessionEventPosition, SessionMessage,
        SessionMessageRole, SessionMessageStatus,
    };

    #[tokio::test]
    async fn history_page_uses_keyset_ordering_unique_keys_and_covering_indexes() {
        let store = StudioStore::open_memory().await.unwrap();
        for turn_sequence in 1..=3 {
            insert_turn(&store, turn_sequence).await;
        }
        insert_item(&store, 31, 3).await;
        insert_item(&store, 30, 3).await;
        insert_item(&store, 20, 2).await;
        insert_item(&store, 10, 1).await;

        let first = store
            .load_session_history_page("session-history", None, 2)
            .await
            .unwrap();
        assert_eq!(
            first
                .turns
                .iter()
                .map(|turn| turn.turn_sequence)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert_eq!(
            first.turns[0]
                .items
                .iter()
                .map(|item| item.sequence)
                .collect::<Vec<_>>(),
            vec![30, 31]
        );
        assert!(first.has_more);
        assert_eq!(first.next_before_turn_sequence, Some(2));

        let second = store
            .load_session_history_page("session-history", first.next_before_turn_sequence, 2)
            .await
            .unwrap();
        assert_eq!(second.turns[0].turn_sequence, 1);
        assert!(!second.has_more);
        assert_eq!(second.next_before_turn_sequence, None);

        let duplicate = history_item_model(32, 3, "history-item-30")
            .insert(store.history_database())
            .await;
        assert!(duplicate.is_err(), "(session_id, item_id) must be unique");

        assert_indexed_without_sort(
            &store,
            "EXPLAIN QUERY PLAN
             SELECT * FROM session_history_turns
             WHERE session_id = ? AND turn_sequence < ?
             ORDER BY turn_sequence DESC LIMIT 3",
            ["session-history".into(), 4_i64.into()],
        )
        .await;
        assert_indexed_without_sort(
            &store,
            "EXPLAIN QUERY PLAN
             SELECT * FROM session_history_items
             WHERE session_id = ? AND turn_id = ?
             ORDER BY sequence ASC",
            ["session-history".into(), "turn-3".into()],
        )
        .await;
    }

    #[tokio::test]
    async fn history_gc_keeps_failed_jobs_and_is_idempotent_after_retry() {
        let store = StudioStore::open_memory().await.unwrap();
        insert_turn(&store, 1).await;
        insert_item(&store, 10, 1).await;
        session_history_checkpoint::ActiveModel {
            session_id: Set("session-history".to_string()),
            revision: Set(1),
            through_sequence: Set(10),
            context_json: Set("[]".to_string()),
            created_at: Set(10),
        }
        .insert(store.history_database())
        .await
        .unwrap();
        history_gc_job::ActiveModel {
            id: Set("history-gc-test".to_string()),
            session_id: Set("session-history".to_string()),
            requested_at: Set(10),
        }
        .insert(store.database())
        .await
        .unwrap();
        store
            .history_database()
            .execute_unprepared(
                "CREATE TRIGGER fail_history_delete
                 BEFORE DELETE ON session_history_items
                 BEGIN SELECT RAISE(FAIL, 'blocked by test'); END",
            )
            .await
            .unwrap();

        assert!(store.run_history_gc_jobs().await.is_err());
        assert!(
            history_gc_job::Entity::find_by_id("history-gc-test".to_string())
                .one(store.database())
                .await
                .unwrap()
                .is_some()
        );

        store
            .history_database()
            .execute_unprepared("DROP TRIGGER fail_history_delete")
            .await
            .unwrap();
        store.run_history_gc_jobs().await.unwrap();
        store.run_history_gc_jobs().await.unwrap();
        assert!(
            history_gc_job::Entity::find_by_id("history-gc-test".to_string())
                .one(store.database())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            session_history_item::Entity::find()
                .filter(session_history_item::Column::SessionId.eq("session-history"))
                .all(store.history_database())
                .await
                .unwrap(),
            Vec::new()
        );
    }

    async fn insert_turn(store: &StudioStore, turn_sequence: i64) {
        session_history_turn::ActiveModel {
            session_id: Set("session-history".to_string()),
            turn_sequence: Set(turn_sequence),
            turn_id: Set(format!("turn-{turn_sequence}")),
            status: Set("completed".to_string()),
            model_json: Set(Some("\"test-model\"".to_string())),
            error_json: Set(None),
            started_at: Set(turn_sequence * 10),
            completed_at: Set(Some(turn_sequence * 10 + 1)),
        }
        .insert(store.history_database())
        .await
        .unwrap();
    }

    async fn insert_item(store: &StudioStore, sequence: i64, turn_sequence: i64) {
        history_item_model(sequence, turn_sequence, &format!("history-item-{sequence}"))
            .insert(store.history_database())
            .await
            .unwrap();
    }

    fn history_item_model(
        sequence: i64,
        turn_sequence: i64,
        item_id: &str,
    ) -> session_history_item::ActiveModel {
        let event = SessionEventEnvelope {
            event_id: item_id.to_string(),
            session_id: "session-history".to_string(),
            source_agent_id: Some("agent-history".to_string()),
            turn_id: Some(format!("turn-{turn_sequence}")),
            emitted_at: sequence,
            position: SessionEventPosition::Durable {
                sequence: u64::try_from(sequence).unwrap(),
            },
            kind: SessionEventKind::MessageChanged {
                message: Box::new(SessionMessage {
                    message_id: format!("message-{sequence}"),
                    session_id: "session-history".to_string(),
                    turn_id: format!("turn-{turn_sequence}"),
                    role: SessionMessageRole::Assistant,
                    status: SessionMessageStatus::Completed,
                    created_at: sequence,
                    updated_at: sequence,
                    completed_at: Some(sequence),
                    error: None,
                    metadata: serde_json::json!({}),
                }),
            },
        };
        session_history_item::ActiveModel {
            session_id: Set("session-history".to_string()),
            sequence: Set(sequence),
            item_id: Set(item_id.to_string()),
            turn_id: Set(format!("turn-{turn_sequence}")),
            item_kind: Set("messageChanged".to_string()),
            payload_json: Set(serde_json::to_string(&event).unwrap()),
            created_at: Set(sequence),
        }
    }

    async fn assert_indexed_without_sort<const N: usize>(
        store: &StudioStore,
        sql: &str,
        values: [sea_orm::Value; N],
    ) {
        let plan = store
            .history_database()
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                sql,
                values,
            ))
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.try_get::<String>("", "detail").unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(plan.contains("SEARCH"), "unexpected query plan: {plan}");
        assert!(
            !plan.contains("TEMP B-TREE"),
            "query sorted in temp: {plan}"
        );
    }
}

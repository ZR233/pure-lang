use anyhow::{Context, Result};
use pl_protocol::{ThreadContextDisposition, ThreadItem, ThreadTurnHistory, ThreadTurnPage, Turn};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::studio::entity::{item, turn};
use crate::studio::store::StudioStore;

impl StudioStore {
    /// 按 turn ordinal 倒序 keyset 分页。cursor 是业务锚点（turn id）：
    /// 取锚点 turn 的 ordinal 做 before 过滤，客户端可从已加载窗口的
    /// 首条内容直接派生回源位置，无需理解或存储服务端编码。
    pub(crate) async fn list_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ThreadTurnPage> {
        let limit = limit.clamp(1, 200);
        let mut query = turn::Entity::find()
            .filter(turn::Column::ThreadId.eq(thread_id))
            .order_by_desc(turn::Column::Ordinal);
        if let Some(anchor_turn_id) = cursor {
            let anchor_ordinal = turn::Entity::find()
                .filter(turn::Column::Id.eq(anchor_turn_id))
                .filter(turn::Column::ThreadId.eq(thread_id))
                .select_only()
                .column(turn::Column::Ordinal)
                .into_tuple::<i64>()
                .one(&self.db)
                .await?
                .with_context(|| format!("unknown Thread turn cursor {anchor_turn_id}"))?;
            query = query.filter(turn::Column::Ordinal.lt(anchor_ordinal));
        }
        let mut models = query
            .limit(u64::try_from(limit.saturating_add(1))?)
            .all(&self.db)
            .await?;
        let has_more = models.len() > limit;
        if has_more {
            models.pop();
        }

        let mut turns = Vec::with_capacity(models.len());
        let recovery = self.conversation_recovery_state(thread_id).await?;
        let rolled_back = recovery
            .rolled_back_turn_ranges
            .iter()
            .flat_map(|range| range.turn_ids.iter().map(String::as_str))
            .collect::<std::collections::BTreeSet<_>>();
        for model in &models {
            let items = item::Entity::find()
                .filter(item::Column::ThreadId.eq(thread_id))
                .filter(item::Column::TurnId.eq(model.id.clone()))
                .order_by_asc(item::Column::Ordinal)
                .all(&self.db)
                .await?
                .into_iter()
                .map(ThreadItem::try_from)
                .collect::<Result<Vec<_>, _>>()?;
            turns.push(ThreadTurnHistory {
                turn: turn_record(model)?,
                items,
                context_disposition: if rolled_back.contains(model.id.as_str()) {
                    ThreadContextDisposition::RolledBack
                } else {
                    ThreadContextDisposition::Active
                },
            });
        }
        let next_cursor = has_more
            .then(|| models.last().map(|turn| turn.id.clone()))
            .flatten();
        Ok(ThreadTurnPage { turns, next_cursor })
    }
}

fn turn_record(model: &turn::Model) -> Result<Turn> {
    Ok(model.clone().try_into()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StudioMode;
    use sea_orm::{ActiveModelTrait, Set};

    async fn seed_turn(store: &StudioStore, thread_id: &str, ordinal: i64) {
        let state = pl_protocol::TurnState::Completed(pl_protocol::CompletedTurnState::new(
            Some(ordinal),
            ordinal,
            pl_protocol::TurnCompletion::Normal,
        ));
        turn::ActiveModel {
            id: Set(format!("turn-{ordinal}")),
            thread_id: Set(thread_id.to_string()),
            ordinal: Set(ordinal),
            revision: Set(1),
            state_json: Set(serde_json::to_string(&state).unwrap()),
            model_json: Set(None),
            usage_json: Set(serde_json::to_string(&pl_model::TokenUsage::default()).unwrap()),
            metadata_json: Set(None),
            updated_at: Set(ordinal),
            ..Default::default()
        }
        .insert(&store.db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn list_thread_turns_pages_by_turn_id_anchor() {
        let store = StudioStore::open_memory().await.unwrap();
        let workspace = std::env::temp_dir().join("history-anchor-test-workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let project = store.upsert_project(&workspace).await.unwrap();
        let thread = store
            .create_thread(&project.id, "History anchor", StudioMode::Simple)
            .await
            .unwrap();
        for ordinal in 1..=5 {
            seed_turn(&store, &thread.id, ordinal).await;
        }

        // 无锚点：从最新方向取一页；next_cursor 是更旧方向下一页锚点（turn id）。
        let first = store.list_thread_turns(&thread.id, None, 2).await.unwrap();
        let ids: Vec<&str> = first.turns.iter().map(|t| t.turn.id.as_str()).collect();
        assert_eq!(ids, ["turn-5", "turn-4"]);
        assert_eq!(first.next_cursor.as_deref(), Some("turn-4"));

        // 锚点是 before 语义：严格早于锚点 ordinal 的 turns。
        let second = store
            .list_thread_turns(&thread.id, Some("turn-4"), 2)
            .await
            .unwrap();
        let ids: Vec<&str> = second.turns.iter().map(|t| t.turn.id.as_str()).collect();
        assert_eq!(ids, ["turn-3", "turn-2"]);
        assert_eq!(second.next_cursor.as_deref(), Some("turn-2"));

        let last = store
            .list_thread_turns(&thread.id, Some("turn-2"), 2)
            .await
            .unwrap();
        let ids: Vec<&str> = last.turns.iter().map(|t| t.turn.id.as_str()).collect();
        assert_eq!(ids, ["turn-1"]);
        assert_eq!(last.next_cursor, None);

        // 锚点限定同一线程：他线程的同名锚点视为未知。
        let error = store
            .list_thread_turns("thread-other", Some("turn-2"), 2)
            .await;
        assert!(error.is_err());
    }
}

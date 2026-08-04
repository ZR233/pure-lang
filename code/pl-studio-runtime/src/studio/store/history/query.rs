use anyhow::Result;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::studio::entity::{session_history_item, session_history_turn};
use crate::studio::records::{
    SessionHistoryItemRecord, SessionHistoryPageRecord, SessionHistoryTurnRecord,
};
use crate::studio::store::StudioStore;

impl StudioStore {
    pub(crate) async fn load_session_history_page(
        &self,
        session_id: &str,
        before_turn_sequence: Option<i64>,
        limit: usize,
    ) -> Result<SessionHistoryPageRecord> {
        let limit = limit.clamp(1, 200);
        let mut query = session_history_turn::Entity::find()
            .filter(session_history_turn::Column::SessionId.eq(session_id))
            .order_by_desc(session_history_turn::Column::TurnSequence);
        if let Some(before_turn_sequence) = before_turn_sequence {
            query =
                query.filter(session_history_turn::Column::TurnSequence.lt(before_turn_sequence));
        }
        let mut turns = query
            .limit(u64::try_from(limit.saturating_add(1))?)
            .all(&self.history_db)
            .await?;
        let has_more = turns.len() > limit;
        if has_more {
            turns.pop();
        }

        let mut records = Vec::with_capacity(turns.len());
        for turn in turns {
            let items = session_history_item::Entity::find()
                .filter(session_history_item::Column::SessionId.eq(session_id))
                .filter(session_history_item::Column::TurnId.eq(turn.turn_id.clone()))
                .order_by_asc(session_history_item::Column::Sequence)
                .all(&self.history_db)
                .await?
                .into_iter()
                .map(|item| {
                    Ok(SessionHistoryItemRecord {
                        sequence: item.sequence,
                        item_id: item.item_id,
                        turn_id: item.turn_id,
                        item_kind: item.item_kind,
                        payload: serde_json::from_str(&item.payload_json)?,
                        created_at: item.created_at,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            records.push(SessionHistoryTurnRecord {
                turn_sequence: turn.turn_sequence,
                turn_id: turn.turn_id,
                status: turn.status,
                model: turn
                    .model_json
                    .map(|model| serde_json::from_str(&model))
                    .transpose()?,
                error: turn
                    .error_json
                    .map(|error| serde_json::from_str(&error))
                    .transpose()?,
                started_at: turn.started_at,
                completed_at: turn.completed_at,
                items,
            });
        }
        let next_before_turn_sequence = has_more
            .then(|| records.last().map(|turn| turn.turn_sequence))
            .flatten();
        Ok(SessionHistoryPageRecord {
            turns: records,
            next_before_turn_sequence,
            has_more,
        })
    }
}

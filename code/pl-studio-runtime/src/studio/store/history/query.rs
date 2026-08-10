use anyhow::{Context, Result, bail};
use pl_protocol::{
    ThreadContextDisposition, ThreadItem, ThreadTurnHistory, ThreadTurnPage, Turn, TurnPhase,
    TurnState,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::studio::entity::{item, turn};
use crate::studio::store::StudioStore;

impl StudioStore {
    pub(crate) async fn list_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ThreadTurnPage> {
        let limit = limit.clamp(1, 200);
        let before_ordinal = cursor.map(parse_cursor).transpose()?;
        let mut query = turn::Entity::find()
            .filter(turn::Column::ThreadId.eq(thread_id))
            .order_by_desc(turn::Column::Ordinal);
        if let Some(before_ordinal) = before_ordinal {
            query = query.filter(turn::Column::Ordinal.lt(before_ordinal));
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
                .map(|item| serde_json::from_str::<ThreadItem>(&item.payload_json))
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
            .then(|| models.last().map(|turn| encode_cursor(turn.ordinal)))
            .flatten();
        Ok(ThreadTurnPage { turns, next_cursor })
    }
}

fn encode_cursor(ordinal: i64) -> String {
    format!("v1:{ordinal:x}")
}

fn parse_cursor(cursor: &str) -> Result<i64> {
    let value = cursor
        .strip_prefix("v1:")
        .context("invalid Thread cursor")?;
    i64::from_str_radix(value, 16).context("invalid Thread cursor")
}

fn turn_record(model: &turn::Model) -> Result<Turn> {
    let state = match model.status.as_str() {
        "queued" => TurnState::Queued,
        "inProgress" => TurnState::InProgress {
            phase: match model.phase.as_deref().unwrap_or("preparing") {
                "preparing" => TurnPhase::Preparing,
                "thinking" => TurnPhase::Thinking,
                "responding" => TurnPhase::Responding,
                "planning" => TurnPhase::Planning,
                "runningTool" => TurnPhase::RunningTool,
                "waitingInteraction" => TurnPhase::WaitingInteraction,
                "persisting" => TurnPhase::Persisting,
                phase => bail!("unknown Turn phase {phase}"),
            },
        },
        "completed" => TurnState::Completed,
        "failed" => TurnState::Failed {
            reason: model.reason.clone().unwrap_or_default(),
        },
        "interrupted" => TurnState::Interrupted {
            reason: model.reason.clone().unwrap_or_default(),
        },
        status => bail!("unknown Turn status {status}"),
    };
    Ok(Turn {
        id: model.id.clone(),
        thread_id: model.thread_id.clone(),
        state,
        failure: model
            .failure_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        started_at: model.started_at,
        updated_at: model.updated_at,
        completed_at: model.completed_at,
    })
}

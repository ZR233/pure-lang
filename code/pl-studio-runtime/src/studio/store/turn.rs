use crate::StudioTurnStatus;
use anyhow::Result;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::mappers::studio_turn_record;
use crate::studio::records::StudioTurnRecord;
use crate::studio::store::StudioStore;

impl StudioStore {
    pub async fn create_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        status: StudioTurnStatus,
        now: i64,
    ) -> Result<StudioTurnRecord> {
        use entities::turn;
        let existing = turn::Entity::find_by_id(turn_id.to_string())
            .one(&self.db)
            .await?;
        if let Some(existing) = existing {
            return Ok(studio_turn_record(existing));
        }
        let row = turn::ActiveModel {
            id: Set(turn_id.to_string()),
            session_id: Set(session_id.to_string()),
            status: Set(status.as_str().to_string()),
            reason: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            completed_at: Set(None),
        }
        .insert(&self.db)
        .await?;
        Ok(studio_turn_record(row))
    }

    pub async fn set_turn_status(
        &self,
        turn_id: &str,
        status: StudioTurnStatus,
        reason: Option<String>,
        now: i64,
    ) -> Result<Option<StudioTurnRecord>> {
        use entities::turn;
        let Some(existing) = turn::Entity::find_by_id(turn_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let mut active: turn::ActiveModel = existing.into();
        active.status = Set(status.as_str().to_string());
        active.reason = Set(reason);
        active.updated_at = Set(now);
        active.completed_at = Set(if is_terminal_turn_status(status) {
            Some(now)
        } else {
            None
        });
        let row = active.update(&self.db).await?;
        Ok(Some(studio_turn_record(row)))
    }

    pub async fn cancel_unfinished_turns(&self, reason: &str) -> Result<Vec<StudioTurnRecord>> {
        use entities::turn;
        let now = unix_seconds();
        let rows = turn::Entity::find()
            .filter(turn::Column::Status.is_not_in([
                StudioTurnStatus::Completed.as_str(),
                StudioTurnStatus::Failed.as_str(),
                StudioTurnStatus::Cancelled.as_str(),
            ]))
            .all(&self.db)
            .await?;
        let mut cancelled = Vec::new();
        for row in rows {
            let mut active: turn::ActiveModel = row.into();
            active.status = Set(StudioTurnStatus::Cancelled.as_str().to_string());
            active.reason = Set(Some(reason.to_string()));
            active.updated_at = Set(now);
            active.completed_at = Set(Some(now));
            cancelled.push(studio_turn_record(active.update(&self.db).await?));
        }
        Ok(cancelled)
    }
}

fn is_terminal_turn_status(status: StudioTurnStatus) -> bool {
    matches!(
        status,
        StudioTurnStatus::Completed | StudioTurnStatus::Failed | StudioTurnStatus::Cancelled
    )
}

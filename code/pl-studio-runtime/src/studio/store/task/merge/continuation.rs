use anyhow::{Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use super::parse_required_evidence;
use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{MergeStatus, TaskProductSignalClaim, TaskRunPhase};

impl StudioStore {
    pub(crate) async fn claim_merge_completion_continuation(
        &self,
        session_id: &str,
    ) -> Result<Vec<TaskProductSignalClaim>> {
        let tx = self.db.begin().await?;
        let result = async {
            let runs = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
                .filter(entities::task_run::Column::Phase.is_in([
                    TaskRunPhase::Implementing.as_str(),
                    TaskRunPhase::Reworking.as_str(),
                ]))
                .all(&tx)
                .await?;
            let run = match runs.as_slice() {
                [] => return Ok(Vec::new()),
                [run] => run,
                _ => bail!("multiple merge-completable runs found for session"),
            };
            let merges = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::merge_record::Column::Status.eq(MergeStatus::Merged.as_str()))
                .order_by_asc(entities::merge_record::Column::UpdatedAt)
                .order_by_asc(entities::merge_record::Column::Id)
                .all(&tx)
                .await?;
            let mut claimed = Vec::new();
            for merge in merges {
                let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
                if evidence.merge_commit.is_none()
                    || evidence.merge_completion_continuation_requested
                {
                    continue;
                }
                evidence.merge_completion_continuation_requested = true;
                let signal = TaskProductSignalClaim {
                    task_run_id: run.id.clone(),
                    agent_id: merge.agent_id.clone(),
                    signal_id: format!("merge-completion:{}", merge.id),
                };
                let mut active: entities::merge_record::ActiveModel = merge.into();
                active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
                active.updated_at = Set(unix_seconds());
                active.update(&tx).await?;
                claimed.push(signal);
            }
            Ok(claimed)
        }
        .await;
        match result {
            Ok(task_run_id) => {
                tx.commit().await?;
                Ok(task_run_id)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn claim_merge_conflict_continuation(
        &self,
        session_id: &str,
    ) -> Result<Option<TaskProductSignalClaim>> {
        let tx = self.db.begin().await?;
        let result = async {
            let runs = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
                .filter(
                    entities::task_run::Column::Phase.eq(TaskRunPhase::ResolvingConflict.as_str()),
                )
                .all(&tx)
                .await?;
            let run = match runs.as_slice() {
                [] => return Ok(None),
                [run] => run,
                _ => bail!("multiple resolving-conflict runs found for session"),
            };
            let Some(merge) = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::merge_record::Column::Status.eq(MergeStatus::Conflicted.as_str()))
                .order_by_desc(entities::merge_record::Column::UpdatedAt)
                .order_by_desc(entities::merge_record::Column::Id)
                .one(&tx)
                .await?
            else {
                bail!("resolving-conflict run has no conflicted merge record");
            };
            let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
            if evidence.conflict_continuation_requested {
                return Ok(None);
            }
            evidence.conflict_continuation_requested = true;
            let signal = TaskProductSignalClaim {
                task_run_id: run.id.clone(),
                agent_id: merge.agent_id.clone(),
                signal_id: format!("merge-conflict:{}", merge.id),
            };
            let mut active: entities::merge_record::ActiveModel = merge.into();
            active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
            active.updated_at = Set(unix_seconds());
            active.update(&tx).await?;
            Ok(Some(signal))
        }
        .await;
        match result {
            Ok(task_run_id) => {
                tx.commit().await?;
                Ok(task_run_id)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}

use anyhow::{Context, Result, ensure};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, TransactionTrait};

#[cfg(test)]
use sea_orm::{ActiveModelTrait, ActiveValue::Set};

use crate::studio::entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{TaskContinuationResolution, TaskContinuationSnapshot};

impl StudioStore {
    pub(crate) async fn load_task_continuation_resolution(
        &self,
        task_run_id: &str,
    ) -> Result<TaskContinuationResolution> {
        self.load_task_continuation_resolution_inner(
            task_run_id,
            #[cfg(test)]
            None,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn load_task_continuation_resolution_with_barrier(
        &self,
        task_run_id: &str,
        barrier: &ContinuationSnapshotTestBarrier,
    ) -> Result<TaskContinuationResolution> {
        self.load_task_continuation_resolution_inner(task_run_id, Some(barrier))
            .await
    }

    async fn load_task_continuation_resolution_inner(
        &self,
        task_run_id: &str,
        #[cfg(test)] barrier: Option<&ContinuationSnapshotTestBarrier>,
    ) -> Result<TaskContinuationResolution> {
        let tx = self.db.begin().await?;
        let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&tx)
            .await?
            .context("task run not found")
            .and_then(super::task_run_record)?;
        ensure!(run.id == task_run_id, "task continuation run mismatch");
        #[cfg(test)]
        if let Some(barrier) = barrier {
            barrier.pause().await;
        }
        if run.phase.is_terminal() {
            tx.commit().await?;
            return Ok(TaskContinuationResolution::Terminal(Box::new(run)));
        }

        let branch_lease = entities::branch_lease::Entity::find()
            .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .one(&tx)
            .await?
            .map(super::branch_lease_record)
            .context("task branch lease not found")?;
        let work_units = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::work_unit::Column::CreatedAt)
            .order_by_asc(entities::work_unit::Column::Id)
            .all(&tx)
            .await?
            .into_iter()
            .map(super::work_unit::work_unit_record)
            .collect::<Result<Vec<_>>>()?;
        let agent_outcomes = entities::agent_outcome::Entity::find()
            .filter(entities::agent_outcome::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::agent_outcome::Column::CreatedAt)
            .order_by_asc(entities::agent_outcome::Column::Id)
            .all(&tx)
            .await?
            .into_iter()
            .map(super::outcome::agent_outcome_record)
            .collect::<Result<Vec<_>>>()?;
        let merge_records = entities::merge_record::Entity::find()
            .filter(entities::merge_record::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::merge_record::Column::CreatedAt)
            .order_by_asc(entities::merge_record::Column::Id)
            .all(&tx)
            .await?
            .into_iter()
            .map(super::merge::merge_record)
            .collect::<Result<Vec<_>>>()?;
        let review_rounds = entities::review_round::Entity::find()
            .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::review_round::Column::Round)
            .all(&tx)
            .await?
            .into_iter()
            .map(super::review::review_round_record)
            .collect::<Result<Vec<_>>>()?;

        ensure!(
            branch_lease.task_run_id == task_run_id,
            "task continuation branch lease mismatch"
        );
        ensure_exact_children(task_run_id, &work_units, |record| &record.task_run_id)?;
        ensure_exact_children(task_run_id, &agent_outcomes, |record| &record.task_run_id)?;
        ensure_exact_children(task_run_id, &merge_records, |record| &record.task_run_id)?;
        ensure_exact_children(task_run_id, &review_rounds, |record| &record.task_run_id)?;
        tx.commit().await?;

        Ok(TaskContinuationResolution::Active(Box::new(
            TaskContinuationSnapshot {
                run,
                branch_lease,
                work_units,
                agent_outcomes,
                merge_records,
                review_rounds,
            },
        )))
    }

    #[cfg(test)]
    pub(crate) async fn terminalize_task_and_release_lease_for_test(
        &self,
        task_run_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&tx)
            .await?
            .context("task run not found")?;
        let mut active: entities::task_run::ActiveModel = run.into();
        active.phase = Set(crate::studio::task_coordinator::TaskRunPhase::Blocked
            .as_str()
            .into());
        active.status_message = Set(Some("terminalized concurrently".to_string()));
        active.updated_at = Set(crate::studio::ids::unix_seconds());
        active.update(&tx).await?;
        entities::branch_lease::Entity::delete_many()
            .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .exec(&tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct ContinuationSnapshotTestBarrier {
    entered: std::sync::Arc<tokio::sync::Barrier>,
    release: std::sync::Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
impl ContinuationSnapshotTestBarrier {
    pub(crate) fn new() -> Self {
        Self {
            entered: std::sync::Arc::new(tokio::sync::Barrier::new(2)),
            release: std::sync::Arc::new(tokio::sync::Barrier::new(2)),
        }
    }

    async fn pause(&self) {
        self.entered.wait().await;
        self.release.wait().await;
    }

    pub(crate) async fn wait_until_entered(&self) {
        self.entered.wait().await;
    }

    pub(crate) async fn release(&self) {
        self.release.wait().await;
    }
}

fn ensure_exact_children<T>(
    task_run_id: &str,
    records: &[T],
    record_task_run_id: impl Fn(&T) -> &String,
) -> Result<()> {
    ensure!(
        records
            .iter()
            .all(|record| record_task_run_id(record) == task_run_id),
        "task continuation child record mismatch"
    );
    Ok(())
}

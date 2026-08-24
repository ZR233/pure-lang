//! 冷恢复资源清理前的数据库授权屏障。
//!
//! 正常活动 Task 不得调用本模块；该入口只服务启动恢复发现的孤立工作目录。

use anyhow::{Context, Result};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};

use super::work_unit::{apply_work_unit_command, work_unit_record};
use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{TaskWorktreeDisposition, WorkUnitCommand};

impl StudioStore {
    pub(crate) async fn authorize_recovery_cleanup(&self, task_run_id: &str) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("recovery cleanup task run not found")?;
            let _record = super::task_run_record(run)?;

            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?;
            for work_unit in work_units {
                let record = work_unit_record(work_unit.clone())?;
                if !record.kind().is_terminal() {
                    apply_work_unit_command(
                        &tx,
                        work_unit,
                        WorkUnitCommand::Cancel {
                            operation_id: format!("recovery-cleanup:{task_run_id}"),
                            reason: "recovery cleanup requested by user".to_string(),
                            disposition: TaskWorktreeDisposition::CleanupRequested,
                        },
                    )
                    .await?;
                }
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}

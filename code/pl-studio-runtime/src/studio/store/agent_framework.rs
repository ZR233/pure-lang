use anyhow::{Context, Result};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, IntoActiveModel};

use crate::studio::StudioStore;
use crate::studio::entity::thread;
use crate::studio::ids::unix_seconds;

pub(in crate::studio) struct ThreadRuntimeSeed {
    pub thread_revision: u64,
    pub runtime_revision: u64,
    pub event_sequence: u64,
}

pub(super) async fn apply_unregistered_child_fault(
    tx: &sea_orm::DatabaseTransaction,
    fault: &super::directory::UnregisteredChildFault,
) -> Result<()> {
    let row = thread::Entity::find_by_id(&fault.thread_id)
        .one(tx)
        .await?
        .with_context(|| format!("spawn compensation Thread not found: {}", fault.thread_id))?;
    anyhow::ensure!(
        row.runtime_revision.is_none(),
        "unregistered fault targets runtime-owned Thread"
    );
    let mut active = row.into_active_model();
    active.state_json = Set(serde_json::to_string(&fault.state)?);
    active.updated_at = Set(unix_seconds());
    active.update(tx).await?;
    Ok(())
}

impl StudioStore {
    pub(in crate::studio) async fn thread_runtime_seed(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadRuntimeSeed>> {
        let Some(row) = thread::Entity::find_by_id(thread_id).one(&self.db).await? else {
            return Ok(None);
        };
        let event_sequence = u64::try_from(row.event_sequence)?;
        Ok(Some(ThreadRuntimeSeed {
            thread_revision: u64::try_from(row.revision)?,
            runtime_revision: event_sequence.saturating_add(1).max(1),
            event_sequence: event_sequence.saturating_add(1).max(1),
        }))
    }
}

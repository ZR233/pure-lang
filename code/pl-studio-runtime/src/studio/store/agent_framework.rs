use anyhow::{Context, Result};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, IntoActiveModel};

use crate::studio::entity::thread;
use crate::studio::ids::unix_seconds;

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

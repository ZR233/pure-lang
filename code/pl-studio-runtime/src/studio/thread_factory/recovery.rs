//! Current-layout checkpoint loading for cold Threads.
//!
//! Old shared journals remain isolated outside v2. Activation and timeline queries only read
//! current session storage; checkpoints are validated on explicit activation.

use anyhow::Result;
use pl_core::thread::ThreadCheckpoint;

use crate::studio::StudioStore;

pub(crate) async fn load_checkpoint(
    store: &StudioStore,
    thread: &pl_protocol::Thread,
) -> Result<Option<ThreadCheckpoint>> {
    store.state(&thread.id).load().await
}

/// Explicit activation or parent continuation migrates history before decoding restart state.
/// This prepares storage only; it never installs an owner or executes the recovered Thread.
pub(crate) async fn load_checkpoint_for_recovery(
    store: &StudioStore,
    thread: &pl_protocol::Thread,
) -> Result<Option<ThreadCheckpoint>> {
    let history = store.history_writer(&thread.id).await?;
    store.state(&thread.id).load_after_migration(&history).await
}

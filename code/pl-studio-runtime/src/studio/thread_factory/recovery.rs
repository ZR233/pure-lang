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

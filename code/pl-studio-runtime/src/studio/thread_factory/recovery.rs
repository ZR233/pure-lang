//! Current-layout checkpoint loading for cold Threads.
//!
//! Legacy shared journals are converted once by the locked pre-publication migration coordinator
//! (not imported into v2). Activation, timeline queries and recovery only ever read the
//! current `state.toml`; they never replay the retired shared session database.

use anyhow::Result;
use pl_core::thread::ThreadCheckpoint;

use crate::studio::StudioStore;

pub(crate) async fn load_checkpoint(
    store: &StudioStore,
    thread: &pl_protocol::Thread,
) -> Result<Option<ThreadCheckpoint>> {
    store.state(&thread.id).load().await
}

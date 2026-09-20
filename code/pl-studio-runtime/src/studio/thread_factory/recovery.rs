//! Durable settlement before publishing a restored owner, shared with the cold auditor.
use crate::studio::StudioStore;
use anyhow::Result;
use pl_core::thread::journal::ThreadCommit;
use std::sync::Arc;

/// Reads a Thread journal from its own session store and appends the deterministic
/// recovery settlement when one is required. A missing history database is an error:
/// audit and activation never create an empty journal for an existing Thread.
pub(in crate::studio) async fn recover_journal(
    store: &StudioStore,
    id: &str,
) -> Result<Vec<Arc<ThreadCommit>>> {
    Ok(store.sessions().recover_thread_journal(id).await?)
}

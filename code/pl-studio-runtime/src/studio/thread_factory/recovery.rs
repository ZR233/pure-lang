//! Durable settlement before publishing a restored owner, shared with the cold auditor.
use crate::studio::StudioStore;
use anyhow::Result;
use pl_core::thread::{
    cold::ColdStore,
    journal::{self, ThreadCommit},
};
use std::sync::Arc;

pub(in crate::studio) async fn recover_journal(
    store: &StudioStore,
    id: &str,
) -> Result<Vec<Arc<ThreadCommit>>> {
    let mut history = store.sessions().read_thread_journal(id).await?;
    if let Some(commit) = journal::recovery_commit(&history)? {
        store
            .sessions()
            .admit(id, commit.sequence, commit.encode()?)?;
        // Publication cannot overtake durable settlement. Admission is atomic; cancellation
        // leaves the store-owned writer responsible for this same immutable commit.
        ColdStore::flush(store.sessions(), id, commit.sequence).await?;
        history.push(Arc::new(commit));
    }
    Ok(history)
}

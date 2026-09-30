//! The realtime observer submits only diagnostic/performance projections.
use super::super::ModelPerformanceOwner;
use anyhow::Result;
use pl_core::thread::ThreadEffectBatch;

pub(super) fn record(
    owner: &ModelPerformanceOwner,
    root: &str,
    commit: &ThreadEffectBatch,
) -> Result<()> {
    for fact in crate::studio::thread_projection::billing_facts(commit)? {
        if fact.auxiliary {
            owner.record_auxiliary_inference(root, &commit.thread_id, &fact.record, fact.status)?;
        } else {
            owner.record_inference(root, &commit.thread_id, &fact.record, fact.status)?;
        }
    }
    Ok(())
}

//! Committed context reductions are distinct from auxiliary inference accounting.
use super::{ProjectionError, order};
use pl_core::thread::{
    ContextReplacementReason, ThreadEffectBatch, ThreadSnapshot, extensions::ExtensionChange,
};
use pl_protocol::{ThreadContextCompactionItem, ThreadItem, ThreadItemState};
use std::sync::Arc;

pub(super) fn receipt(
    payload: &pl_core::context::OpaquePayload,
) -> Result<Option<crate::compaction::CompactionReceipt>, ProjectionError> {
    if payload.format() != "pl.studio.compaction" {
        return Ok(None);
    }
    if payload.version() != 1 {
        return Err(ProjectionError::UnsupportedOutput(
            "unsupported compaction receipt version".into(),
        ));
    }
    Ok(Some(serde_json::from_str(payload.content())?))
}

pub(super) fn project_compactions(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadEffectBatch>],
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let mut items = Vec::new();
    for commit in journal
        .iter()
        .filter(|commit| commit.sequence <= snapshot.commit_sequence)
    {
        if !commit
            .replacements
            .iter()
            .any(|replacement| replacement.reason == ContextReplacementReason::Compaction)
        {
            continue;
        }
        for extension in commit.extensions.iter() {
            let ExtensionChange::Put { id, record } = extension else {
                continue;
            };
            let (turn_id, state) = match receipt(&record.payload) {
                Ok(Some(receipt)) if receipt.implementation.is_some() => (
                    receipt.turn_id,
                    ThreadItemState::ContextCompaction(ThreadContextCompactionItem::new(
                        None,
                        None,
                        commit.committed_at,
                    )),
                ),
                Ok(Some(_)) | Ok(None) => continue,
                Err(error) => (
                    String::new(),
                    ThreadItemState::Raw(pl_protocol::ThreadRawItem {
                        payloads: vec![super::raw_payload(&record.payload)],
                        notice: error.to_string(),
                        recorded_at: commit.committed_at,
                    }),
                ),
            };
            items.push(ThreadItem::new(
                order::compaction_id(id),
                thread_id.into(),
                turn_id,
                0,
                record.revision,
                commit.committed_at,
                commit.committed_at,
                state,
            ));
        }
    }
    Ok(items)
}

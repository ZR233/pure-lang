//! 批次成功落库后的耐久修订推进与屏障完成。

use std::collections::HashMap;

use super::handle::WriterShared;
use super::queue::{PendingBatch, QueueEntry, QueuedMutation, StudioMutation};

pub(super) fn advance_batch_durability(shared: &WriterShared, batch: &PendingBatch) {
    let mut revisions = HashMap::<String, u64>::new();
    for entry in &batch.entries {
        match entry {
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => {
                revisions
                    .entry(commit.agent_id.to_string())
                    .and_modify(|revision| *revision = (*revision).max(commit.facts.revision))
                    .or_insert(commit.facts.revision);
            }
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(_),
                ..
            })
            | QueueEntry::Barrier(_) => {}
        }
    }
    if revisions.is_empty() {
        return;
    }
    let mut changed = false;
    if !revisions.is_empty() {
        let mut durable = shared
            .durable_revisions
            .lock()
            .expect("durable revision lock poisoned");
        for (owner_id, revision) in revisions {
            let current = durable.entry(owner_id).or_default();
            if revision > *current {
                *current = revision;
                changed = true;
            }
        }
    }
    if changed {
        let next = shared.durable_progress.borrow().wrapping_add(1);
        shared.durable_progress.send_replace(next);
    }
}

pub(super) fn advance_durable_revision(shared: &WriterShared, owner_id: &str, revision: u64) {
    let mut durable = shared
        .durable_revisions
        .lock()
        .expect("durable revision lock poisoned");
    let current = durable.entry(owner_id.to_string()).or_default();
    if revision <= *current {
        return;
    }
    *current = revision;
    drop(durable);
    let next = shared.durable_progress.borrow().wrapping_add(1);
    shared.durable_progress.send_replace(next);
}

/// 成功批次只需要完成显式耐久化屏障；commit 入队时已经返回。
pub(super) fn complete_applied_batch(batch: PendingBatch) {
    for entry in batch.entries {
        match entry {
            QueueEntry::Mutation(_) => {}
            QueueEntry::Barrier(sender) => {
                let _ = sender.send(Ok(()));
            }
        }
    }
}

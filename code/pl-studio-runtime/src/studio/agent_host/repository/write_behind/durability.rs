//! Completion of product persistence barriers.
use super::queue::{PendingBatch, QueueEntry};

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

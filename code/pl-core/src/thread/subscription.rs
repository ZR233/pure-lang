//! Coalescing snapshot observations, separate from the lossless commit journal.
use super::*;

/// Read-only live snapshots. Intermediate notifications may coalesce; use commit_sequence to page history.
/// Dropping this subscription neither cancels execution nor closes the Thread.
#[derive(Debug)]
pub struct ThreadSubscription {
    pub(super) snapshots: watch::Receiver<ThreadSnapshot>,
    pub(super) initial: bool,
}
impl ThreadSubscription {
    /// Returns the current snapshot first, then newer snapshots until the owner closes.
    /// Cancellation of this wait does not consume a snapshot that has not been returned.
    pub async fn next(&mut self) -> Option<ThreadSnapshot> {
        if self.initial {
            self.initial = false;
            return Some(self.snapshots.borrow_and_update().clone());
        }
        self.snapshots.changed().await.ok()?;
        Some(self.snapshots.borrow_and_update().clone())
    }
}

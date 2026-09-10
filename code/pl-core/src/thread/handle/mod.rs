//! Bounded command submission and independent snapshot/interrupt handles.
use super::*;
mod commands;
mod inputs;
mod operations;
pub(super) use commands::Command;

/// A bounded command handle; mutable Thread state is owned exclusively by its actor.
#[derive(Debug, Clone)]
pub struct ThreadHandle {
    _lifetime: Arc<HandleLifetime>,
    commands: mpsc::Sender<Command>,
    mailbox: mpsc::Sender<super::mailbox::MailboxCommand>,
    interrupt: cancellation::InterruptHandle,
    snapshots: watch::Receiver<ThreadSnapshot>,
    history: Arc<std::sync::RwLock<Vec<Arc<journal::ThreadCommit>>>>,
}

#[derive(Debug)]
struct HandleLifetime(cancellation::InterruptHandle);
impl Drop for HandleLifetime {
    fn drop(&mut self) {
        self.0.begin_close();
    }
}

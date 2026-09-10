//! A narrow live cancellation capability; canonical execution state remains actor-owned.
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
pub(super) struct InterruptHandle(Arc<Mutex<InterruptState>>);

#[derive(Debug, Default)]
struct InterruptState {
    active: Option<CancellationToken>,
    closing: bool,
}

impl InterruptHandle {
    pub(super) fn activate(&self, parent: &CancellationToken) -> ActiveExecution {
        let token = parent.child_token();
        {
            let mut state = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.closing {
                token.cancel();
            }
            state.active = Some(token.clone());
        }
        ActiveExecution {
            handle: self.clone(),
            token,
        }
    }
    pub(super) fn begin_close(&self) {
        let token = {
            let mut state = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.closing = true;
            state.active.clone()
        };
        if let Some(token) = token {
            token.cancel();
        }
    }
    pub(super) fn is_closing(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closing
    }

    pub(super) fn interrupt(&self) -> bool {
        let token = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .clone();
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }
}

pub(super) struct ActiveExecution {
    handle: InterruptHandle,
    pub(super) token: CancellationToken,
}
impl Drop for ActiveExecution {
    fn drop(&mut self) {
        self.handle
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interruption_is_generation_scoped_and_does_not_cancel_the_callers_token() {
        let handle = InterruptHandle::default();
        let parent = CancellationToken::new();
        assert!(!handle.interrupt());
        let first = handle.activate(&parent);
        assert!(handle.interrupt());
        assert!(first.token.is_cancelled());
        assert!(!parent.is_cancelled());
        drop(first);
        assert!(!handle.interrupt());
        let second = handle.activate(&parent);
        assert!(!second.token.is_cancelled());
        parent.cancel();
        assert!(second.token.is_cancelled());
    }
}

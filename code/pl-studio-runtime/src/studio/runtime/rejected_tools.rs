//! Owns failed candidate closures across refresh retries and cancelled shutdown waiters.
use pl_core::{
    thread::ThreadHandle,
    tool::opaque::{RejectedTools, ToolError},
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct RejectedToolOwners {
    entries: Arc<Mutex<Vec<Arc<Entry>>>>,
    stop: Arc<Mutex<tokio_util::sync::CancellationToken>>,
}
struct Entry {
    thread: ThreadHandle,
    gate: tokio::sync::Mutex<()>,
    resources: Mutex<Option<Box<RejectedTools>>>,
}
struct Lease {
    entry: Arc<Entry>,
    resources: Option<Box<RejectedTools>>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(resources) = self.resources.take() {
            *self
                .entry
                .resources
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(resources);
        }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("rejected tool candidate cleanup failed: {0}")]
struct CleanupFailure(#[source] Arc<ToolError>);

impl RejectedToolOwners {
    pub(super) fn start(&self) -> tokio_util::sync::CancellationToken {
        let token = tokio_util::sync::CancellationToken::new();
        *self
            .stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = token.clone();
        token
    }
    pub(super) fn stop(&self) {
        self.stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cancel();
    }
    pub(super) fn retain(&self, thread: ThreadHandle, resources: Box<RejectedTools>) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::new(Entry {
                thread,
                gate: Default::default(),
                resources: Mutex::new(Some(resources)),
            }));
    }
    pub(super) async fn retry(&self, thread: Option<&ThreadHandle>) -> anyhow::Result<()> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|entry| thread.is_none_or(|thread| thread.same_instance(&entry.thread)))
            .cloned()
            .collect::<Vec<_>>();
        for entry in entries {
            // This permit serializes cleanup operations; the resource data lock never spans IO.
            let _permit = entry.gate.lock().await;
            let resources = entry
                .resources
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let mut lease = Lease {
                entry: entry.clone(),
                resources,
            };
            if let Some(resources) = &mut lease.resources {
                resources.retry_close().await.map_err(CleanupFailure)?;
                lease.resources = None;
            }
            self.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|current| !Arc::ptr_eq(current, &entry));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload,
        model::*,
        tool::opaque::{CallContext, Registration, Tool},
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct NoModel;
    impl ModelSession for NoModel {
        async fn prepare(&mut self, _: ModelRequest) -> Result<PreparedModelCall, ModelError> {
            panic!("cleanup cannot invoke a model")
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    #[derive(Debug)]
    struct Closing {
        fail: AtomicBool,
        block: AtomicBool,
        calls: AtomicUsize,
        entered: tokio::sync::Notify,
    }
    #[derive(Debug)]
    struct Candidate(Arc<Closing>);
    impl Tool for Candidate {
        async fn execute(
            &self,
            _: OpaquePayload,
            _: CallContext,
        ) -> Result<pl_core::tool::ToolOutput, ToolError> {
            panic!("rejected candidate cannot execute")
        }
        async fn close(&self) -> Result<(), ToolError> {
            self.0.calls.fetch_add(1, Ordering::SeqCst);
            self.0.entered.notify_one();
            if self.0.block.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if self.0.fail.load(Ordering::SeqCst) {
                return Err(ToolError::new(std::io::Error::other("retry cleanup")));
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn failed_cleanup_survives_cancelled_retry_and_stop_until_original_owner_closes() {
        let thread = ThreadHandle::start("closed".into(), DynModelSession::new(NoModel)).unwrap();
        thread.close().await.unwrap();
        let closing = Arc::new(Closing {
            fail: AtomicBool::new(true),
            block: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            entered: Default::default(),
        });
        let error = thread
            .register_tools(vec![
                Registration::new(
                    "candidate".into(),
                    OpaquePayload::text("candidate"),
                    Candidate(closing.clone()),
                )
                .unwrap(),
            ])
            .await
            .unwrap_err();
        let pl_core::thread::ThreadError::RejectedTools(resources) = error else {
            panic!("expected owned failed candidate");
        };
        let owners = RejectedToolOwners::default();
        let stopping = owners.start();
        owners.retain(thread.clone(), resources);
        assert!(owners.retry(Some(&thread)).await.is_err());
        closing.block.store(true, Ordering::SeqCst);
        // Consume the previous close's notification before observing the blocked attempt.
        closing.entered.notified().await;
        let retry = tokio::spawn({
            let owners = owners.clone();
            async move { owners.retry(None).await }
        });
        closing.entered.notified().await;
        retry.abort();
        assert!(retry.await.unwrap_err().is_cancelled());
        owners.stop();
        assert!(stopping.is_cancelled());
        assert_eq!(owners.entries.lock().unwrap().len(), 1);
        assert!(
            owners.entries.lock().unwrap()[0]
                .resources
                .lock()
                .unwrap()
                .is_some()
        );
        closing.block.store(false, Ordering::SeqCst);
        closing.fail.store(false, Ordering::SeqCst);
        owners.retry(None).await.unwrap();
        assert!(owners.entries.lock().unwrap().is_empty());
        assert_eq!(closing.calls.load(Ordering::SeqCst), 4);
        owners.retry(None).await.unwrap();
        assert_eq!(closing.calls.load(Ordering::SeqCst), 4);
    }
}

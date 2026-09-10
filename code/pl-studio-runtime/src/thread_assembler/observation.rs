//! Product observation registration; callbacks execute outside the registry lock.
use super::*;

type Observe = dyn Fn(String, ThreadHandle) + Send + Sync;
type Drain = dyn Fn(String) -> futures::future::BoxFuture<'static, Result<(), ThreadAssemblyError>>
    + Send
    + Sync;
#[derive(Clone)]
pub(super) struct AssemblyObservation(Arc<Observe>, Arc<Drain>);
impl std::fmt::Debug for AssemblyObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AssemblyObservation")
    }
}
impl StudioThreadAssembler {
    pub(crate) fn observe_assembly(
        &self,
        observe: impl Fn(String, ThreadHandle) + Send + Sync + 'static,
        drain: impl Fn(String) -> futures::future::BoxFuture<'static, Result<(), ThreadAssemblyError>>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), ThreadAssemblyError> {
        let mut state = self.0.state();
        if state.closing
            || !state.entries.is_empty()
            || !state.creating.is_empty()
            || state.observation.is_some()
        {
            return Err(ThreadAssemblyError::Closed);
        }
        state.observation = Some(AssemblyObservation(Arc::new(observe), Arc::new(drain)));
        Ok(())
    }
    pub(super) fn publish_observation(&self, id: &str, thread: &ThreadHandle) {
        let observer = self.0.state().observation.clone();
        if let Some(observer) = observer {
            (observer.0)(id.to_owned(), thread.clone());
        }
    }
    pub(super) async fn drain_observation(&self, id: &str) -> Result<(), ThreadAssemblyError> {
        let observer = self.0.state().observation.clone();
        if let Some(observer) = observer {
            (observer.1)(id.to_owned()).await?;
        }
        Ok(())
    }
    pub(crate) async fn notify_parent(
        &self,
        id: &str,
        message: pl_core::thread::inbox::ThreadMessage,
    ) -> Result<(), ThreadAssemblyError> {
        let target = {
            let state = self.0.state();
            if state.closing {
                return Ok(());
            }
            Some(
                state
                    .entries
                    .get(id)
                    .ok_or_else(|| ThreadAssemblyError::Identity(id.into()))?,
            )
            .filter(|entry| {
                entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open
            })
            .map(|entry| (entry.thread.clone(), entry.execution))
        };
        let Some((thread, execution)) = target else {
            return Ok(());
        };
        match thread.send_message_and_resume(message, execution).await {
            Ok(_) => Ok(()),
            Err(ThreadError::Closed) if thread.snapshot().lifecycle != ThreadLifecycle::Open => {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }
}

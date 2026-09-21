//! Nonblocking effect admission and explicit durability barriers for Thread persistence.
use super::*;
use futures::future::BoxFuture;
use std::{fmt, future::Future, sync::Arc};

/// One fixed persistence ticket. The checkpoint may only publish after the effect fence is durable.
#[derive(Debug, Clone)]
pub struct ThreadWrite {
    pub effect: Arc<ThreadEffectBatch>,
    pub checkpoint: ThreadCheckpoint,
}

/// Storage failure retains its typed source; it never rolls back a Thread commit.
#[derive(Debug, thiserror::Error)]
#[error("cold store failed: {source}")]
pub struct ColdStoreError {
    #[source]
    pub source: Box<dyn std::error::Error + Send + Sync>,
}

/// Retained queued bytes reported by a backend; completed but unsaved work may exceed limits.
#[derive(Debug, Clone, Default)]
pub struct StoragePressure {
    pub thread_bytes: u64,
    pub store_bytes: u64,
    pub error: Option<Arc<ColdStoreError>>,
}

/// One durable tool-task fact read back from the host store.
///
/// It is the authority a repeated read-only task query uses after the owner has released the
/// finished result from its transient window: the task identity/status come from the host's durable
/// call facts and `delivery` carries the full committed result body when the call reached a
/// terminal delivery. It is a read of durable facts, never a resident second history.
#[derive(Debug, Clone)]
pub struct DurableToolTask {
    pub task: super::task::TaskRecord,
    pub delivery: Option<ToolDelivery>,
}

/// Injected asynchronous sink. `admit` only queues immutable effect data and must not block.
pub trait ColdStore: Send + Sync + fmt::Debug + 'static {
    /// Reports current queue pressure without blocking on IO.
    fn pressure(&self, _thread_id: &str) -> StoragePressure {
        StoragePressure::default()
    }
    /// Accepts exactly this encoded effect batch; repeated admission is idempotent.
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError>;
    /// Waits for previously admitted records to become durable, retaining failed writes for retry.
    fn flush(
        &self,
        thread_id: &str,
        sequence: u64,
    ) -> impl Future<Output = Result<(), ColdStoreError>> + Send;
    /// Reads one finished tool task by its core task identity from the durable store.
    ///
    /// A backend that keeps no task facts returns `Ok(None)`. Returning a non-terminal record means
    /// the delivery is not durable yet; callers must not present that as a finished result.
    fn read_tool_task(
        &self,
        _thread_id: &str,
        _task_id: &str,
    ) -> impl Future<Output = Result<Option<DurableToolTask>, ColdStoreError>> + Send {
        async { Ok(None) }
    }
}
trait ErasedColdStore: Send + Sync + fmt::Debug {
    fn pressure(&self, thread_id: &str) -> StoragePressure;
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError>;
    fn flush<'a>(
        &'a self,
        thread_id: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<(), ColdStoreError>>;
    fn read_tool_task<'a>(
        &'a self,
        thread_id: &'a str,
        task_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<DurableToolTask>, ColdStoreError>>;
}
impl<T: ColdStore> ErasedColdStore for T {
    fn pressure(&self, thread_id: &str) -> StoragePressure {
        ColdStore::pressure(self, thread_id)
    }
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError> {
        ColdStore::admit(self, thread_id, write)
    }
    fn flush<'a>(
        &'a self,
        thread_id: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<(), ColdStoreError>> {
        Box::pin(ColdStore::flush(self, thread_id, sequence))
    }
    fn read_tool_task<'a>(
        &'a self,
        thread_id: &'a str,
        task_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<DurableToolTask>, ColdStoreError>> {
        Box::pin(ColdStore::read_tool_task(self, thread_id, task_id))
    }
}

/// Shared store handle; Thread ownership and mutable model/tool instances remain private.
#[derive(Debug, Clone)]
pub struct ColdStoreHandle(Arc<dyn ErasedColdStore>);
impl ColdStoreHandle {
    /// Erases one backend; independent Threads may share its asynchronous writer.
    pub fn new(store: impl ColdStore) -> Self {
        Self(Arc::new(store))
    }

    /// Reads one finished tool task from the durable store, if the backend keeps task facts.
    pub(crate) async fn read_tool_task(
        &self,
        thread_id: &str,
        task_id: &str,
    ) -> Result<Option<DurableToolTask>, ColdStoreError> {
        self.0.read_tool_task(thread_id, task_id).await
    }
}

/// Persistence progress is observable separately from authoritative runtime facts.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceState {
    pub attached: bool,
    pub pressure_paused: bool,
    pub pressure_warning: bool,
    pub pending_thread_bytes: u64,
    pub pending_store_bytes: u64,
    pub admitted_sequence: u64,
    pub durable_sequence: u64,
    pub error: Option<String>,
}

impl Owner {
    pub(super) fn refresh_storage_pressure(&mut self) {
        let pressure = match &self.cold {
            Some(store) => store.0.pressure(&self.id),
            // Without an attached store every published batch stays unadmitted below, so the same
            // byte budget still gates admission. The accepted facts are never dropped to make room
            // for new work; the Thread reports pressure and pauses further admission instead of
            // letting a transient window grow without bound.
            None => StoragePressure::default(),
        };
        // A synchronous `admit` rejection is a fact about *these* effects: they were published but
        // the store did not take them, so a healthy pressure report from a background writer must
        // not erase it. Otherwise the Thread would keep executing model work whose facts are not
        // handed over — and a rejected attempt could be silently corrected instead of failing closed.
        // Only when every published effect really was handed over does a healthy report release a
        // latch that only reflected a transient background conflict.
        let handed_over = match self.pending_effects.front() {
            Some(pending) => {
                pending.write.effect.sequence <= self.state.persistence.admitted_sequence
            }
            None => true,
        };
        match &pressure.error {
            Some(error) => {
                self.state.persistence.error = Some(error.to_string());
                self.cold_error = Some(error.clone());
            }
            None if handed_over => {
                self.state.persistence.error = None;
                self.cold_error = None;
            }
            None => {}
        }
        // A queue drained by an attached store costs nothing here; only a queue that really holds
        // undurable batches is measured, and each of those batches is serialized at most once.
        let unadmitted = if self.pending_effects.is_empty() {
            0
        } else {
            self.pending_effects
                .iter_mut()
                .fold(0_u64, |total, pending| {
                    let bytes = match pending.encoded_bytes {
                        Some(bytes) => bytes,
                        None => {
                            // An unencodable batch keeps the pessimistic `u64::MAX` weight, exactly
                            // like the previous fold-based accounting did.
                            let bytes = pending
                                .write
                                .effect
                                .encode()
                                .map_or(u64::MAX, |payload| payload.content().len() as u64);
                            pending.encoded_bytes = Some(bytes);
                            bytes
                        }
                    };
                    total.saturating_add(bytes)
                })
        };
        let thread = pressure.thread_bytes.saturating_add(unadmitted);
        let total = pressure.store_bytes.saturating_add(unadmitted);
        let state = &mut self.state.persistence;
        state.pending_thread_bytes = thread;
        state.pending_store_bytes = total;
        if thread >= 64 * 1024 * 1024 || total >= 256 * 1024 * 1024 {
            state.pressure_paused = true;
        } else if thread <= 32 * 1024 * 1024 && total <= 128 * 1024 * 1024 {
            state.pressure_paused = false;
        }
        state.pressure_warning = thread >= 32 * 1024 * 1024 || total >= 128 * 1024 * 1024;
    }

    pub(super) fn admit_cold(&mut self) {
        let Some(store) = &self.cold else {
            return;
        };
        self.state.persistence.attached = true;
        while let Some(pending) = self.pending_effects.front() {
            let sequence = pending.write.effect.sequence;
            match store.0.admit(&self.id, pending.write.clone()) {
                Ok(()) => {
                    self.state.persistence.admitted_sequence = sequence;
                    self.pending_effects.pop_front();
                }
                Err(error) => {
                    self.state.persistence.error = Some(error.to_string());
                    self.cold_error = Some(Arc::new(error));
                    return;
                }
            }
        }
        self.state.persistence.error = None;
        self.cold_error = None;
    }

    pub(super) async fn flush_cold(&mut self) -> Result<(), ThreadError> {
        self.admit_cold();
        if let Some(error) = &self.cold_error {
            self.publish_snapshot();
            return Err(ThreadError::Storage(error.clone()));
        }
        let Some(store) = &self.cold else {
            return Ok(());
        };
        let result = store
            .0
            .flush(&self.id, self.state.persistence.admitted_sequence)
            .await;
        match result {
            Ok(()) => {
                self.state.persistence.durable_sequence = self.state.persistence.admitted_sequence;
                // The durable handoff makes every covered commit recoverable from history, so the
                // live window may release those bodies once its byte budget needs the space.
                self.effect_window
                    .release_through(self.state.persistence.durable_sequence);
            }
            Err(error) => {
                // A caller that asked for a fixed durable target and did not get it has a terminal
                // fact: answer with it directly instead of letting the pressure mirror below
                // mistake the store's momentary report for recovery.
                let error = Arc::new(error);
                self.state.persistence.error = Some(error.to_string());
                self.cold_error = Some(error.clone());
                self.publish_snapshot();
                return Err(ThreadError::Storage(error));
            }
        }
        self.refresh_storage_pressure();
        self.publish_snapshot();
        match &self.cold_error {
            Some(error) => Err(ThreadError::Storage(error.clone())),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextContent;
    use crate::model::{
        DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput, PreparedModelCall,
    };
    use crate::thread::{RuntimeFact, StepInput, ThreadError, ThreadHandle};
    use tokio_util::sync::CancellationToken;

    /// One store whose reported pressure error can be turned on and off, modelling a writer that hit
    /// a transient conflict and then recovered.
    #[derive(Debug)]
    struct ToggleStore(Arc<std::sync::atomic::AtomicBool>);

    impl ColdStore for ToggleStore {
        fn pressure(&self, _: &str) -> StoragePressure {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                StoragePressure {
                    error: Some(Arc::new(ColdStoreError {
                        source: Box::new(std::io::Error::other(
                            "Execution Error: error returned from database: (code: 5) \
                             database is locked",
                        )),
                    })),
                    ..StoragePressure::default()
                }
            } else {
                StoragePressure::default()
            }
        }

        fn admit(&self, _: &str, _: ThreadWrite) -> Result<(), ColdStoreError> {
            Ok(())
        }

        async fn flush(&self, _: &str, _: u64) -> Result<(), ColdStoreError> {
            Ok(())
        }
    }

    struct Echo;

    impl ModelSession for Echo {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: Vec::new(),
                    tool_calls: Vec::new(),
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }

        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    fn step_input(turn: &str, attempt: &str) -> StepInput {
        StepInput {
            turn_id: turn.into(),
            attempt_id: attempt.into(),
            content: Vec::new(),
            cancellation: CancellationToken::new(),
        }
    }

    /// 现场回归：一次瞬时锁冲突被锁存后，写者一报告恢复就必须解除，不得继续阻断模型准入。
    ///
    /// 该用例对修复前的行为有区分度：旧的"只增不减"压力锁存在第二步仍会返回 `Storage`。
    #[tokio::test]
    async fn a_recovered_writer_reopens_model_admission() {
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .attach_storage(ColdStoreHandle::new(ToggleStore(busy.clone())))
            .await
            .unwrap();

        // 写者报告冲突时，准入失败闭锁，且不产生任何模型尝试。
        assert!(matches!(
            thread.step(step_input("turn-1", "attempt-1")).await,
            Err(ThreadError::Storage(_))
        ));
        assert!(thread.snapshot().attempts.is_empty());

        // 写者恢复（压力里不再有错误）后，准入重新生效；第二个 Turn 不再被锁存的瞬时错误阻断。
        busy.store(false, std::sync::atomic::Ordering::SeqCst);
        let recovered = thread.step(step_input("turn-2", "attempt-2")).await;
        assert!(
            recovered.is_ok(),
            "恢复后的模型准入仍被锁存的瞬时错误阻断: {recovered:?}"
        );
        thread.close().await.unwrap();
    }

    /// One store whose *synchronous* admission verdict can be flipped while its pressure stays healthy.
    #[derive(Debug)]
    struct RejectOnce(Arc<std::sync::atomic::AtomicBool>);

    impl ColdStore for RejectOnce {
        fn admit(&self, _: &str, _: ThreadWrite) -> Result<(), ColdStoreError> {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                Err(ColdStoreError {
                    source: Box::new(std::io::Error::other("synchronous admission rejected")),
                })
            } else {
                Ok(())
            }
        }

        async fn flush(&self, _: &str, _: u64) -> Result<(), ColdStoreError> {
            Ok(())
        }
    }

    /// 同步 `admit` 拒绝在 pressure 健康时也不得被镜像清除；只有它真正被 durable 接手后才解封。
    ///
    /// 该用例对修复前的行为有区分度：旧的"pressure 健康即清除"会让第二步正常跑起来（Storage 断言
    /// 失败），从而把一次未被 durable 接手的拒绝降级为可继续、甚至可纠错的普通失败。
    #[tokio::test]
    async fn a_synchronous_admission_rejection_survives_a_healthy_pressure_report() {
        let rejecting = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .attach_storage(ColdStoreHandle::new(RejectOnce(rejecting.clone())))
            .await
            .unwrap();

        // 一次已发布的 effect 被同步拒绝：它还没有被 durable 接手。
        thread
            .update_facts(vec![RuntimeFact {
                source_id: "plugin".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("fact"),
                }],
            }])
            .await
            .unwrap();
        assert!(thread.snapshot().persistence.error.is_some());

        // 后台压力这里是健康的（`pressure()` 无错误），但同步拒绝必须继续阻断模型准入。
        let blocked = thread.step(step_input("turn-1", "attempt-1")).await;
        assert!(
            matches!(blocked, Err(ThreadError::Storage(_))),
            "同步拒绝被健康 pressure 抹掉了: {blocked:?}"
        );
        assert!(thread.snapshot().attempts.is_empty());

        // durable 真正接手（flush 重试成功）之后，准入重新生效。
        rejecting.store(false, std::sync::atomic::Ordering::SeqCst);
        thread.flush().await.unwrap();
        let admitted = thread.step(step_input("turn-2", "attempt-2")).await;
        assert!(admitted.is_ok(), "durable 接手后准入仍被阻断: {admitted:?}");
        thread.close().await.unwrap();
    }
}

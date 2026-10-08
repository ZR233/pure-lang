//! pre-active 启动进度订阅：独立于 installed bridge 的 typed watch。
//!
//! `RustLib.init` 之后、`startStudioRuntime` 之前即可订阅；订阅必须携带
//! `prepareStartupAttempt` 发放的令牌，首帧是本尝试的当前真实阶段，`Ready`/`Failed`
//! 是终态。新一代尝试开始后，旧观察者直接终止、不接收新世代阶段（design/18 §18.3）。
//! 句柄不要求 active bridge，也不借用 runtime 的 shutdown 令牌：取消与释放完全由
//! 订阅自己的令牌承担。shutdown 开始即失效当前尝试后，未读的 `Ready` 终态不会
//! 迟到交付为新的成功。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio_util::sync::CancellationToken;

use super::{
    BridgeEventSubscription, BridgeSubscriptionInner, BridgeSubscriptionKind, OWNED_PENDING,
    SubscriptionCompletionFacts, SubscriptionOwner, owned_completion,
};
use crate::api::studio::handlers::{startup_attempt_active, subscribe_startup_attempt};
use crate::api::studio::types::{BridgeError, BridgeStartupStage};
use crate::frb_generated::StreamSink;

/// 订阅一次启动尝试的进度；可在 bridge 安装前调用。
///
/// # Errors
/// 令牌未知、过期或其尝试已终结（Failed/Stopped 之后）时明确拒绝；已 Ready 的当前
/// 令牌可订阅，首帧即 `Ready` 终态。
pub async fn subscribe_startup_progress(
    attempt: u64,
) -> Result<BridgeEventSubscription, BridgeError> {
    let events = subscribe_startup_attempt(attempt)?;
    Ok(BridgeEventSubscription {
        inner: Arc::new(BridgeSubscriptionInner {
            // pre-active 订阅没有 installed bridge，也就没有注册表 id；句柄独立取消。
            id: 0,
            kind: BridgeSubscriptionKind::Startup { attempt },
            cancel: CancellationToken::new(),
            producer_task: tokio::sync::Mutex::new(None),
            sink_task: tokio::sync::Mutex::new(None),
            thread_receiver: tokio::sync::Mutex::new(None),
            product_topic_receiver: tokio::sync::Mutex::new(None),
            shutdown_receiver: tokio::sync::Mutex::new(None),
            startup_receiver: tokio::sync::Mutex::new(Some(events)),
            facts: Arc::new(SubscriptionCompletionFacts::default()),
        }),
    })
}

impl BridgeEventSubscription {
    /// 打开启动进度流；首帧为本尝试的当前阶段，`Ready`/`Failed` 后结束。
    ///
    /// 新一代尝试开始后流直接终止：旧观察者不接收新世代阶段。
    ///
    /// # Errors
    /// 拒绝其他订阅种类或第二个流消费者。
    pub async fn startup_stream(
        &self,
        sink: StreamSink<BridgeStartupStage>,
    ) -> Result<(), BridgeError> {
        let attempt = match self.inner.kind {
            BridgeSubscriptionKind::Startup { attempt } => attempt,
            _ => {
                return Err(BridgeError::invalid_argument(
                    "non-startup subscription cannot open a startup stream",
                ));
            }
        };
        let mut events = self
            .inner
            .startup_receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| {
                BridgeError::invalid_argument("startup stream can only be opened once")
            })?;
        let cancel = self.inner.cancel.clone();
        let id = self.inner.id;
        let facts = Arc::clone(&self.inner.facts);
        let mut task = self.inner.sink_task.lock().await;
        if cancel.is_cancelled() {
            return Ok(());
        }
        facts.sink.store(OWNED_PENDING, Ordering::SeqCst);
        let sink_task = tokio::spawn(async move {
            // 首帧总是当前真实阶段：订阅建立与阶段推进之间没有轮询窗口。
            let (frame_attempt, stage) = {
                let frame = events.borrow_and_update();
                (frame.attempt, frame.stage)
            };
            if frame_attempt != attempt {
                return;
            }
            if stale_ready(attempt, stage) {
                return;
            }
            if sink.add(stage).is_err() || is_terminal(stage) {
                return;
            }
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    changed = events.changed() => if changed.is_err() { break; },
                }
                let (frame_attempt, stage) = {
                    let frame = events.borrow_and_update();
                    (frame.attempt, frame.stage)
                };
                // 新一代尝试开始：旧观察者终止，不转发新世代阶段。
                if frame_attempt != attempt {
                    break;
                }
                if stale_ready(attempt, stage) {
                    break;
                }
                if sink.add(stage).is_err() {
                    break;
                }
                if is_terminal(stage) {
                    break;
                }
            }
        });
        *task = Some(owned_completion(
            sink_task,
            id,
            facts,
            SubscriptionOwner::Sink,
        ));
        Ok(())
    }
}

/// shutdown 已把本尝试落为 Stopped 时，迟到的 `Ready` 终态不作为新成功交付。
fn stale_ready(attempt: u64, stage: BridgeStartupStage) -> bool {
    matches!(stage, BridgeStartupStage::Ready) && !startup_attempt_active(attempt)
}

fn is_terminal(stage: BridgeStartupStage) -> bool {
    matches!(
        stage,
        BridgeStartupStage::Ready | BridgeStartupStage::Failed
    )
}

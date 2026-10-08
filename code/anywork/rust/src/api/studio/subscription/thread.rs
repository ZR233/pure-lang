//! Thread 状态流订阅：Turn、活动、交互与运行时帧；内容由 ChatView 窗口交付。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::mpsc;

use super::{
    BridgeEventSubscription, BridgeSubscriptionInner, BridgeSubscriptionKind,
    BridgeThreadStreamEnvelope, OWNED_PENDING, SubscriptionCompletionFacts, SubscriptionOwner,
    owned_completion, send_or_cancel,
};
use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::thread_stream::bridge_thread_update;
use crate::api::studio::types::BridgeError;
use crate::frb_generated::StreamSink;

/// 订阅一个 Thread 的**状态流**：Turn、活动、交互、运行时与 lagged 帧。
///
/// 内容不在状态流里：条目与流式正文只由 ChatView 窗口交付。内容帧的过滤发生在生产端
/// （runtime 的状态流不再转发 `Item*`/`Delta`），所以这里没有“末端丢弃内容帧”的开关，客户端
/// 也不会再因为过滤内容帧而看到 revision 空洞。状态流的 envelope `revision` 只按状态帧递增，
/// 与 ChatView 窗口的内容 revision 相互独立。
pub async fn subscribe_thread(thread_id: String) -> Result<BridgeEventSubscription, BridgeError> {
    let bridge = active_bridge().await?;
    let mut events = bridge
        .studio
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: thread_id.clone(),
        })
        .await?;
    let cancel = bridge.shutdown.child_token();
    let producer_cancel = cancel.clone();
    let (sender, receiver) = mpsc::channel(128);
    // 订阅 = 观察者注册：producer 存活期间钉住驻留，防止 LRU 淘汰正在被
    // 观察的线程（淘汰后该订阅流会永久静默——bus 无事件也无关闭信号）。
    let residency_pin = bridge.studio.pin_thread(&thread_id);
    let producer_task = tokio::spawn(async move {
        let _residency_pin = residency_pin;
        loop {
            let frame = tokio::select! {
                _ = producer_cancel.cancelled() => break,
                frame = events.recv() => match frame {
                    Ok(Some(frame)) => frame,
                    Ok(None) => break,
                    Err(error) => {
                        let _ = send_or_cancel(
                            &producer_cancel,
                            &sender,
                            BridgeThreadStreamEnvelope::Failure {
                                error: BridgeError::from(error),
                            },
                        )
                        .await;
                        break;
                    }
                },
            };
            match bridge_thread_update(frame) {
                Ok(Some(update)) => {
                    if !send_or_cancel(
                        &producer_cancel,
                        &sender,
                        BridgeThreadStreamEnvelope::Data {
                            update: Box::new(update),
                        },
                    )
                    .await
                    {
                        break;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = send_or_cancel(
                        &producer_cancel,
                        &sender,
                        BridgeThreadStreamEnvelope::Failure {
                            error: BridgeError::from(error),
                        },
                    )
                    .await;
                    break;
                }
            }
        }
        // 终态 Closed 尽力交付：取消与满队列都不得让 cancel_and_wait 永久等待。
        let _ = send_or_cancel(
            &producer_cancel,
            &sender,
            BridgeThreadStreamEnvelope::Closed,
        )
        .await;
    });
    let id = bridge.subscriptions.next_id();
    let facts = Arc::new(SubscriptionCompletionFacts::default());
    facts.producer.store(OWNED_PENDING, Ordering::SeqCst);
    let inner = Arc::new(BridgeSubscriptionInner {
        id,
        kind: BridgeSubscriptionKind::Thread { thread_id },
        cancel,
        producer_task: tokio::sync::Mutex::new(Some(owned_completion(
            producer_task,
            id,
            Arc::clone(&facts),
            SubscriptionOwner::Producer,
        ))),
        sink_task: tokio::sync::Mutex::new(None),
        thread_receiver: tokio::sync::Mutex::new(Some(receiver)),
        product_topic_receiver: tokio::sync::Mutex::new(None),
        shutdown_receiver: tokio::sync::Mutex::new(None),
        startup_receiver: tokio::sync::Mutex::new(None),
        facts,
    });
    bridge.subscriptions.register(&inner).await;
    Ok(BridgeEventSubscription { inner })
}

impl BridgeEventSubscription {
    /// 打开该 Thread 订阅的状态流；每个订阅只能打开一次。
    ///
    /// # Errors
    /// 拒绝其他订阅种类或第二个流消费者。
    pub async fn thread_stream(
        &self,
        sink: StreamSink<BridgeThreadStreamEnvelope>,
    ) -> Result<(), BridgeError> {
        if !matches!(self.inner.kind, BridgeSubscriptionKind::Thread { .. }) {
            return Err(BridgeError::invalid_argument(
                "non-Thread subscription cannot open a Thread stream",
            ));
        }
        let mut receiver = self
            .inner
            .thread_receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| {
                BridgeError::invalid_argument("Thread stream can only be opened once")
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
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    envelope = receiver.recv() => {
                        let Some(envelope) = envelope else {
                            break;
                        };
                        if sink.add(envelope).is_err() {
                            cancel.cancel();
                            break;
                        }
                    }
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

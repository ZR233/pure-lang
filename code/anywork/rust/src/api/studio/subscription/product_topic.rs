//! typed 产品 topic 订阅：首帧基线 + 该领域事件；作用域由 topic 表达。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::mpsc;

use super::{
    BridgeEventSubscription, BridgeSubscriptionInner, BridgeSubscriptionKind, OWNED_PENDING,
    SubscriptionCompletionFacts, SubscriptionOwner, owned_completion, send_or_cancel,
};
use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::event::{bridge_product_baseline, bridge_product_event};
use crate::api::studio::types::{
    BridgeError, BridgeProductTopic, BridgeProductTopicStreamEnvelope,
};
use crate::frb_generated::StreamSink;
use pl_studio_runtime::StudioProductFrame;

/// 订阅一个 typed 产品 topic。
///
/// 一次订阅只观察一个领域：runtime 先登记该 topic 的接收者，再读取领域基线；本模块
/// producer 把首帧 `Baseline` 与后续事件转发给 Dart sink，`Lagged` 帧提示客户端仅重读
/// 该 topic 的基线（无 durable replay）。
pub async fn create_product_topic_subscription(
    topic: BridgeProductTopic,
) -> Result<BridgeEventSubscription, BridgeError> {
    topic.validate_scope()?;
    let bridge = active_bridge().await?;
    let runtime_topic = topic.to_runtime();
    let producer_topic = topic.clone();
    let mut events = bridge.studio.subscribe_product_topic(runtime_topic).await?;
    let cancel = bridge.shutdown.child_token();
    let producer_cancel = cancel.clone();
    let (sender, receiver) = mpsc::channel(64);
    let producer_task = tokio::spawn(async move {
        let topic = producer_topic;
        loop {
            let frame = tokio::select! {
                _ = producer_cancel.cancelled() => break,
                frame = events.recv() => match frame {
                    Ok(Some(frame)) => frame,
                    Ok(None) | Err(_) => break,
                },
            };
            let envelope = match frame {
                StudioProductFrame::Baseline(baseline) => {
                    match bridge_product_baseline(*baseline) {
                        Ok((revision, state)) => BridgeProductTopicStreamEnvelope::Baseline {
                            topic: topic.clone(),
                            revision,
                            state: Box::new(state),
                        },
                        Err(error) => {
                            tracing::warn!(
                                error_bytes = error.to_string().len(),
                                "failed to convert Studio product baseline"
                            );
                            continue;
                        }
                    }
                }
                StudioProductFrame::Event(event) => match bridge_product_event(*event) {
                    Ok(event) => BridgeProductTopicStreamEnvelope::Data {
                        event: Box::new(event),
                    },
                    Err(error) => {
                        tracing::warn!(
                            error_bytes = error.to_string().len(),
                            "failed to convert Studio product event"
                        );
                        continue;
                    }
                },
                StudioProductFrame::Lagged { dropped, .. } => {
                    BridgeProductTopicStreamEnvelope::Lagged {
                        topic: topic.clone(),
                        dropped,
                    }
                }
            };
            if !send_or_cancel(&producer_cancel, &sender, envelope).await {
                break;
            }
        }
        // 终态 Closed 尽力交付：取消与满队列都不得让 cancel_and_wait 永久等待。
        let _ = send_or_cancel(
            &producer_cancel,
            &sender,
            BridgeProductTopicStreamEnvelope::Closed,
        )
        .await;
    });
    let id = bridge.subscriptions.next_id();
    let facts = Arc::new(SubscriptionCompletionFacts::default());
    facts.producer.store(OWNED_PENDING, Ordering::SeqCst);
    let inner = Arc::new(BridgeSubscriptionInner {
        id,
        kind: BridgeSubscriptionKind::ProductTopic { topic },
        cancel,
        producer_task: tokio::sync::Mutex::new(Some(owned_completion(
            producer_task,
            id,
            Arc::clone(&facts),
            SubscriptionOwner::Producer,
        ))),
        sink_task: tokio::sync::Mutex::new(None),
        thread_receiver: tokio::sync::Mutex::new(None),
        product_topic_receiver: tokio::sync::Mutex::new(Some(receiver)),
        shutdown_receiver: tokio::sync::Mutex::new(None),
        startup_receiver: tokio::sync::Mutex::new(None),
        facts,
    });
    bridge.subscriptions.register(&inner).await;
    Ok(BridgeEventSubscription { inner })
}

impl BridgeEventSubscription {
    /// 打开该 topic 订阅的产品流；每个订阅只能打开一次。
    ///
    /// # Errors
    /// 拒绝其他订阅种类或第二个流消费者。
    pub async fn product_topic_stream(
        &self,
        sink: StreamSink<BridgeProductTopicStreamEnvelope>,
    ) -> Result<(), BridgeError> {
        if !matches!(self.inner.kind, BridgeSubscriptionKind::ProductTopic { .. }) {
            return Err(BridgeError::invalid_argument(
                "non-product subscription cannot open a product topic stream",
            ));
        }
        let mut receiver = self
            .inner
            .product_topic_receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| {
                BridgeError::invalid_argument("product topic stream can only be opened once")
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

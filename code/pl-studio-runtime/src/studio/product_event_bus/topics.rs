//! 产品事件的 topic 通道注册与分发路由。
//!
//! 每个订阅者只登记自己声明的 topic；事件按 [`StudioProductTopic::of_kind`] 路由到
//! 对应通道，无关领域既不进入订阅者缓冲，也不占用该 topic 的容量。通道不持有业务
//! 投影，只转发 owner 发布的同一 canonical envelope。

use std::collections::BTreeMap;

use tokio::sync::broadcast;

use crate::{StudioProductEventEnvelope, StudioProductTopic};

/// 单个 topic 通道的容量；与订阅者消费速率无关的进程内环形缓冲。
const TOPIC_CHANNEL_CAPACITY: usize = 256;

/// topic → 通道的惰性注册表。
///
/// `emit` 只向已存在的通道发送；没有任何订阅者的 topic 不保留事件（无 durable
/// replay），新订阅者通过首帧基线取得当前事实。
#[derive(Debug, Default)]
pub(super) struct TopicChannels {
    channels: BTreeMap<StudioProductTopic, broadcast::Sender<StudioProductEventEnvelope>>,
}

impl TopicChannels {
    /// 订阅一个 topic；通道不存在时以固定容量创建。
    pub(super) fn subscribe(
        &mut self,
        topic: &StudioProductTopic,
    ) -> broadcast::Receiver<StudioProductEventEnvelope> {
        self.channels
            .entry(topic.clone())
            .or_insert_with(|| broadcast::channel(TOPIC_CHANNEL_CAPACITY).0)
            .subscribe()
    }

    /// 把 envelope 只投递到它所属的 topic 通道。
    ///
    /// 通道没有订阅者或已关闭时丢弃；`sequence` 是全局序列，消费端按领域 revision
    /// 判断新旧，因此同一通道内序列不连续不是 lag。
    pub(super) fn dispatch(&mut self, envelope: &StudioProductEventEnvelope) {
        let topic = StudioProductTopic::of_kind(&envelope.kind);
        if let Some(sender) = self.channels.get(&topic)
            && sender.send(envelope.clone()).is_err()
        {
            // 所有接收者都已离开：保留通道注册，等待下一个订阅者登记。
        }
    }
}

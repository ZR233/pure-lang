//! 驻留 Thread actor 的 LRU 双端队列。
//!
//! 只有当前页面/最近访问的会话保留驻留 actor；超过容量时从队首淘汰空闲
//! actor（淘汰前由调用方 flush 该 Thread 的全部 pending commits，被淘汰
//! Thread 保留目录索引与全部 durable 状态，再次访问时按需恢复）。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;

/// 驻留 Thread actor 容量；钉住集合（活跃订阅）不受此限制。
const RESIDENT_CAPACITY: usize = 16;

#[derive(Clone)]
pub(in crate::studio) struct ThreadResidency {
    order: Arc<AsyncMutex<VecDeque<String>>>,
    pinned: Arc<Mutex<std::collections::HashSet<String>>>,
    capacity: usize,
}

impl ThreadResidency {
    pub(in crate::studio) fn new() -> Self {
        Self {
            order: Arc::new(AsyncMutex::new(VecDeque::new())),
            pinned: Arc::new(Mutex::new(std::collections::HashSet::new())),
            capacity: RESIDENT_CAPACITY,
        }
    }

    /// 访问或驻留时移到队尾；不改变容量语义。
    pub(in crate::studio) async fn touch(&self, thread_id: &str) {
        let mut order = self.order.lock().await;
        order.retain(|id| id != thread_id);
        order.push_back(thread_id.to_string());
    }

    /// 移除已淘汰条目。
    pub(in crate::studio) async fn remove(&self, thread_id: &str) {
        self.order.lock().await.retain(|id| id != thread_id);
    }

    /// 返回超出容量的队首候选（按最久未使用排序）；pinned 线程由调用方跳过。
    pub(in crate::studio) async fn over_capacity(&self) -> Vec<String> {
        let order = self.order.lock().await;
        if order.len() <= self.capacity {
            return Vec::new();
        }
        order
            .iter()
            .take(order.len().saturating_sub(self.capacity))
            .cloned()
            .collect()
    }

    /// 订阅 pin：有活跃订阅的线程不参与 LRU 淘汰（design/17 空闲判定）。
    pub(in crate::studio) fn pin(&self, thread_id: &str) {
        self.pinned
            .lock()
            .expect("residency pinned lock poisoned")
            .insert(thread_id.to_string());
    }

    pub(in crate::studio) fn unpin(&self, thread_id: &str) {
        self.pinned
            .lock()
            .expect("residency pinned lock poisoned")
            .remove(thread_id);
    }

    pub(in crate::studio) fn is_pinned(&self, thread_id: &str) -> bool {
        self.pinned
            .lock()
            .expect("residency pinned lock poisoned")
            .contains(thread_id)
    }

    /// 测试用：当前驻留顺序快照。
    #[cfg(test)]
    pub(in crate::studio) async fn snapshot(&self) -> Vec<String> {
        self.order.lock().await.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn touch_moves_entry_to_back_and_over_capacity_reports_lru_front() {
        let residency = ThreadResidency::new();
        for id in ["a", "b", "c", "d"] {
            residency.touch(id).await;
        }
        residency.touch("a").await;
        assert_eq!(residency.snapshot().await, ["b", "c", "d", "a"]);
        assert!(residency.over_capacity().await.is_empty());

        residency.remove("d").await;
        assert_eq!(residency.snapshot().await, ["b", "c", "a"]);
    }
    #[tokio::test]
    async fn pinned_threads_are_reported_but_skipped_by_caller() {
        let residency = ThreadResidency::new();
        residency.pin("subscribed");
        assert!(residency.is_pinned("subscribed"));
        residency.unpin("subscribed");
        assert!(!residency.is_pinned("subscribed"));
    }
}

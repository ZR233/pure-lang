//! 驻留 Thread actor 的 LRU 双端队列。
//!
//! 只有当前页面/最近访问的会话保留驻留 actor；超过容量时从队首淘汰空闲
//! actor（淘汰前由调用方 flush 该 Thread 的全部 pending commits，被淘汰
//! Thread 保留目录索引与全部 durable 状态，再次访问时按需恢复）。

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::Mutex;

/// 驻留 actor 容量；钉住集合（pending input/活动 Task 等）不受此限制。
const RESIDENT_CAPACITY: usize = 16;

#[derive(Clone)]
pub(in crate::studio) struct ThreadResidency {
    order: Arc<Mutex<VecDeque<String>>>,
    capacity: usize,
}

impl ThreadResidency {
    pub(in crate::studio) fn new() -> Self {
        Self {
            order: Arc::new(Mutex::new(VecDeque::new())),
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

    /// 返回超出容量的队首候选（按最久未使用排序）。
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
}

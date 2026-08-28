//! 驻留 Thread actor 的 LRU 双端队列。
//!
//! 只有当前页面/最近访问的会话保留驻留 actor；超过容量时从队首淘汰空闲
//! actor（淘汰前由调用方 flush 该 Thread 的全部 pending commits，被淘汰
//! Thread 保留目录索引与全部 durable 状态，再次访问时按需恢复）。

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;

/// 未选中、无任务、无显式 pin 的驻留 Thread actor 容量。
const INACTIVE_RESIDENT_CAPACITY: usize = 4;

#[derive(Clone)]
pub(in crate::studio) struct ThreadResidency {
    order: Arc<AsyncMutex<VecDeque<String>>>,
    pinned: Arc<Mutex<HashSet<String>>>,
    capacity: usize,
}

pub(in crate::studio) struct ThreadResidencyPins {
    residency: ThreadResidency,
    thread_ids: Vec<String>,
}

impl Drop for ThreadResidencyPins {
    fn drop(&mut self) {
        for thread_id in &self.thread_ids {
            self.residency.unpin(thread_id);
        }
    }
}

impl ThreadResidency {
    pub(in crate::studio) fn new() -> Self {
        Self {
            order: Arc::new(AsyncMutex::new(VecDeque::new())),
            pinned: Arc::new(Mutex::new(HashSet::new())),
            capacity: INACTIVE_RESIDENT_CAPACITY,
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

    /// 返回超出非 pin 容量的队首候选（按最久未使用排序）。
    ///
    /// 活跃订阅/当前选择由内部 pin 表达；非终态 Task 的 root、executor 与 reviewer
    /// 由调用方通过 `task_pins` 传入，二者都不占用 4 个 inactive LRU 名额。
    pub(in crate::studio) async fn over_capacity(
        &self,
        task_pins: &HashSet<String>,
    ) -> Vec<String> {
        let order = self.order.lock().await;
        let pinned = self.pinned.lock().expect("residency pinned lock poisoned");
        let inactive = order
            .iter()
            .filter(|id| !pinned.contains(*id) && !task_pins.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        if inactive.len() <= self.capacity {
            return Vec::new();
        }
        let excess = inactive.len().saturating_sub(self.capacity);
        inactive.into_iter().take(excess).collect()
    }

    /// 订阅 pin：有活跃订阅的线程不参与 LRU 淘汰（design/17 空闲判定）。
    pub(in crate::studio) fn pin(&self, thread_id: &str) {
        self.pinned
            .lock()
            .expect("residency pinned lock poisoned")
            .insert(thread_id.to_string());
    }

    /// 在一个跨 Thread 操作期间临时钉住完整 owner 集合。
    pub(in crate::studio) fn pin_many(
        &self,
        thread_ids: impl IntoIterator<Item = String>,
    ) -> ThreadResidencyPins {
        let thread_ids = thread_ids.into_iter().collect::<Vec<_>>();
        for thread_id in &thread_ids {
            self.pin(thread_id);
        }
        ThreadResidencyPins {
            residency: self.clone(),
            thread_ids,
        }
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
        for id in ["a", "b", "c", "d", "e"] {
            residency.touch(id).await;
        }
        residency.touch("a").await;
        assert_eq!(residency.snapshot().await, ["b", "c", "d", "e", "a"]);
        assert_eq!(residency.over_capacity(&HashSet::new()).await, ["b"]);

        residency.remove("d").await;
        assert_eq!(residency.snapshot().await, ["b", "c", "e", "a"]);
    }
    #[tokio::test]
    async fn pinned_threads_are_reported_but_skipped_by_caller() {
        let residency = ThreadResidency::new();
        residency.pin("subscribed");
        assert!(residency.is_pinned("subscribed"));
        residency.unpin("subscribed");
        assert!(!residency.is_pinned("subscribed"));
    }

    #[tokio::test]
    async fn subscription_and_task_pins_do_not_consume_inactive_capacity() {
        let residency = ThreadResidency::new();
        residency.pin("selected");
        for id in ["selected", "task", "a", "b", "c", "d", "e"] {
            residency.touch(id).await;
        }
        let task_pins = HashSet::from(["task".to_string()]);

        assert_eq!(residency.over_capacity(&task_pins).await, ["a"]);
    }
}

//! Durable timeline pages backed by one independently owned SQLite database per Thread.
use super::StudioRuntime;
use crate::studio::storage::history::HistoryStore;
use anyhow::Result;
use pl_protocol::{ThreadItem, TimelinePage, TimelineQuery, TimelineTurn};
use std::collections::BTreeMap;

impl StudioRuntime {
    /// Returns an item page, including related Turn metadata, without executing the Thread.
    ///
    /// # Errors
    /// Fails on unknown Thread/item identity or canonical storage/projection failure.
    pub async fn list_timeline_items(
        &self,
        thread_id: &str,
        query: TimelineQuery,
        limit: usize,
    ) -> Result<TimelinePage> {
        self.read_owned_thread(thread_id).await?;
        let history = self.ensure_timeline_history(thread_id).await?;
        history.page(&query, limit).await
    }

    /// Reads one complete timeline item by identity, bypassing the page preview budget.
    ///
    /// 普通分页仍然只走 SQL 并把超大条目压成同身份预览；本入口用于按 identity 直接读取完整
    /// payload。它不激活 owner、不 flush writer，也不触发恢复，冷热 Thread 走同一条只读路径。
    ///
    /// # Errors
    /// Fails on unknown Thread/item identity or canonical storage failure.
    pub async fn read_timeline_item(
        &self,
        thread_id: &str,
        item_id: &str,
    ) -> Result<pl_protocol::TimelineItemRead> {
        self.read_owned_thread(thread_id).await?;
        let history = self.ensure_timeline_history(thread_id).await?;
        history.read_item(item_id).await
    }

    /// 只读地打开一个 Thread 的 history 数据库，供普通滚动分页使用。
    ///
    /// 关键约束：不激活 owner、不 flush writer、不触发恢复。需要的持久化栅栏直接来自
    /// 已落盘的 checkpoint（`history_fence`），因此冷 Thread 与热 Thread 走同一条只读路径。
    pub(super) async fn ensure_timeline_history(&self, thread_id: &str) -> Result<HistoryStore> {
        let history = self.store.history(thread_id).await?;
        let thread = self.read_protocol_thread(thread_id).await?;
        let fence = crate::studio::thread_factory::recovery::load_checkpoint(&self.store, &thread)
            .await?
            .map_or(0, |checkpoint| checkpoint.history_fence);
        if history.watermark().await? >= fence {
            return Ok(history);
        }
        anyhow::bail!(
            "Thread history is behind its durable checkpoint fence; pending persistence must recover first"
        )
    }

    /// 该 Thread 的唯一有序历史写者句柄，供实时订阅做 ordinal 预留。
    ///
    /// 它与同一 Thread 的 effect commit 是同一个写者（同一连接状态），实时预留因此不再是一条独立
    /// 的 SQLite writer。订阅的**读**仍走 [`Self::ensure_timeline_history`] 返回的只读句柄，长写事务
    /// 不会把分页或实时读堵在这一条写连接上；没有活跃持有者时这里只登记新句柄（`HistoryStore::open`
    /// 不做 IO），不会创建也不升级数据库。
    pub(super) async fn ensure_timeline_history_writer(
        &self,
        thread_id: &str,
    ) -> Result<HistoryStore> {
        self.store.history_writer(thread_id).await
    }

    /// 订阅/重连的固定持久化屏障。
    ///
    /// 注册事件接收端之后冻结 owner 当前提交 revision，再等 writer 的 history 与
    /// checkpoint 覆盖该固定目标。模型执行中的 owner 不处理 Flush 命令，订阅不能
    /// 通过该命令等待，否则首帧可能被进行中的 provider 流无限期阻塞。
    pub(in crate::studio) async fn await_timeline_barrier(&self, thread_id: &str) -> Result<()> {
        let history = self.store.history(thread_id).await?;
        let thread = self.read_protocol_thread(thread_id).await?;
        let desired = match self.threads.thread(thread_id) {
            Some(handle) => {
                let snapshot = handle.snapshot();
                let desired = snapshot.commit_sequence;
                anyhow::ensure!(
                    snapshot.persistence.admitted_sequence >= desired,
                    "Thread history is behind its admitted effects; pending persistence must recover first"
                );
                if desired > snapshot.persistence.durable_sequence {
                    let mut progress = self.store.thread_persistence().subscribe();
                    loop {
                        let status = progress
                            .borrow()
                            .threads
                            .iter()
                            .find(|status| status.thread_id == thread_id)
                            .cloned();
                        let Some(status) = status else {
                            anyhow::bail!(
                                "Thread persistence writer disappeared before its checkpoint became durable"
                            );
                        };
                        if let Some(error) = status.last_error {
                            anyhow::bail!(
                                "Thread persistence failed before its checkpoint became durable: {error}"
                            );
                        }
                        if status.state_durable_revision.unwrap_or(0) >= desired
                            && status.history_durable_sequence.unwrap_or(0) >= desired
                            && status.calls_durable_sequence.unwrap_or(0) >= desired
                        {
                            break;
                        }
                        progress.changed().await?;
                    }
                }
                desired
            }
            None => crate::studio::thread_factory::recovery::load_checkpoint(&self.store, &thread)
                .await?
                .map_or(0, |checkpoint| checkpoint.state_revision),
        };
        anyhow::ensure!(
            history.watermark().await? >= desired,
            "Thread history is behind its checkpoint fence; pending persistence must recover first"
        );
        Ok(())
    }
}

pub(in crate::studio) fn timeline_turns(
    thread_id: &str,
    state: &pl_core::thread::ThreadSnapshot,
    items: &[ThreadItem],
) -> Vec<TimelineTurn> {
    let ends: BTreeMap<_, _> = items
        .iter()
        .filter(|item| item.kind() != pl_protocol::ThreadItemKind::ContextCompaction)
        .map(|item| (item.turn_id.as_str(), item.id.as_str()))
        .collect();
    let rolled_back = super::history::rolled_back_turns(state);
    items
        .iter()
        .filter_map(|item| {
            let pl_protocol::ThreadItemState::Turn(turn) = item.state() else {
                return None;
            };
            let id = item.turn_id.clone();
            let last_item_id = ends.get(id.as_str())?;
            Some(TimelineTurn {
                turn: pl_protocol::Turn {
                    id: id.clone(),
                    thread_id: thread_id.into(),
                    input_id: turn.input_id().map(str::to_owned),
                    revision: item.revision,
                    state: turn.state().clone(),
                    updated_at: item.updated_at,
                },
                last_item_id: (*last_item_id).into(),
                context_disposition: if rolled_back.contains(&id) {
                    pl_protocol::ThreadContextDisposition::RolledBack
                } else {
                    pl_protocol::ThreadContextDisposition::Active
                },
            })
        })
        .collect()
}

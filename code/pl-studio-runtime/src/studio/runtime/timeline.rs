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
    /// 调用时机固定在事件接收端注册之后、首个历史窗口读取之前：先把活跃 owner 的当前
    /// 草稿推进 writer 并等待固定 ticket 落盘，再确认数据库水位覆盖 owner 已提交的
    /// revision。只有屏障通过后，`list_timeline_items(Latest)` 才是权威且无缺口的窗口。
    pub(in crate::studio) async fn await_timeline_barrier(&self, thread_id: &str) -> Result<()> {
        let history = self.store.history(thread_id).await?;
        let thread = self.read_protocol_thread(thread_id).await?;
        let desired = match self.threads.thread(thread_id) {
            Some(handle) => {
                // 固定目标必须在 flush 之前取样：`flush` 处理命令时才冻结自己的 ticket，因此它
                // 保证的 durable 水位一定覆盖这里取到的 revision。若先 flush 再取 snapshot，
                // flush 期间被受理的新 effect 会把 `desired` 抬到屏障覆盖范围之外，让一次成功的
                // 持久化反而被判定成“待恢复的持久化失败”。屏障之后受理的 effect 本来就不在这
                // 个窗口里：订阅已在屏障之前注册，它们按 live 事件到达。
                let desired = handle.snapshot().commit_sequence;
                handle.flush().await?;
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

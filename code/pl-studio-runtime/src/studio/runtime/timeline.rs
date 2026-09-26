//! **已落盘历史**分页：每个 Thread 一份独立 SQLite（`history.sqlite`）的纯 SQL 读取。
//!
//! 这里只服务“数据库里已经写了哪些条目”的长历史分页与按 identity 回读 —— 输入再长也稳定、可分页、
//! 不激活 owner、不 flush writer。它**不**是实时内容窗口：尚未落盘的流式条目不在其中，读取水位也只
//! 是 history 的 applied write sequence。实时窗口（初始/分页/按身份读）是
//! [`super::chat_window`]（HTTP `/window`），两者共享同一个 `ChatSession` 事实源，但职责不同。
use super::StudioRuntime;
use crate::studio::storage::history::HistoryStore;
use anyhow::Result;
use pl_core::chat::{ChatFocus, ChatView, Session};
use pl_protocol::{TimelinePage, TimelineQuery};

impl StudioRuntime {
    /// Opens a read-only chat view without activating the Thread or waiting for a checkpoint.
    pub async fn open_chat(&self, thread_id: &str, focus: ChatFocus) -> Result<ChatView> {
        self.chat_session(thread_id)
            .await?
            .open_chat(focus)
            .await
            .map_err(Into::into)
    }

    /// Shares the same in-memory timeline with any live view or future producer.
    pub(in crate::studio::runtime) async fn chat_session(
        &self,
        thread_id: &str,
    ) -> Result<Session> {
        self.read_owned_thread(thread_id).await?;
        self.store.chat_session(thread_id).await
    }

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
}

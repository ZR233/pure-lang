//! `StudioStore` 上的轻量会话目录入口。
//!
//! 冷分页、搜索、归档范围与关联补全都只消费 `catalog.toml`，不读取任何会话
//! `state.toml`/`history.sqlite`（design/17 §17.1、§17.2）。目录 mutation 的唯一
//! 命令通道是 `ProductEventBus::commit_directory`：它同步写入本模块的 `catalog.toml`
//! 事实，write-behind 队列再把同一 delta 异步落到 SQLite 镜像并幂等重放 TOML。

use std::collections::HashSet;

use anyhow::Result;
use pl_protocol::Thread;
use pl_protocol::studio::ThreadDirectoryQuery;

use crate::studio::catalog::CatalogEntry;
use crate::studio::store::StudioStore;
use crate::studio::store::directory::{DirectoryDelta, ThreadDirectoryCursor};

impl StudioStore {
    /// 未归档 root 会话的冷分页（`(updated_at, id)` 降序 keyset）。
    pub(in crate::studio) async fn catalog_page_active(
        &self,
        cursor: Option<&ThreadDirectoryCursor>,
        limit: usize,
    ) -> Result<Vec<Thread>> {
        let key = cursor.map(|cursor| (cursor.updated_at, cursor.id.clone()));
        Ok(self.catalog().page_active(key.as_ref(), limit))
    }

    /// 带搜索/过滤的冷分页；`project_matches` 是 Project 名称/路径命中的 id 集合。
    pub(in crate::studio) async fn catalog_query(
        &self,
        query: &ThreadDirectoryQuery,
        project_matches: &HashSet<String>,
        cursor: Option<&ThreadDirectoryCursor>,
        limit: usize,
    ) -> Result<Vec<Thread>> {
        let key = cursor.map(|cursor| (cursor.updated_at, cursor.id.clone()));
        Ok(self
            .catalog()
            .page(query, project_matches, key.as_ref(), limit))
    }

    /// 把一次目录 delta 幂等写入 canonical TOML 目录事实源（`workspaces.toml` + `catalog.toml`）。
    ///
    /// 这是目录摘要持久化的唯一实现：命令通道（`ProductEventBus::commit_directory`）同步调用
    /// 它保证发布 revision 前事实已落盘，write-behind 批次在 [`super::directory::apply_directory_delta`]
    /// 中异步调用同一实现，使观察热路径的状态摘要经现有 writer/revision 机制合并落盘。
    /// 各写入彼此幂等：内容一致时不推进 revision、不落盘；命令与观察 delta 按 FIFO 顺序
    /// 合并，最终目录以最后提交的命令为准。
    pub(in crate::studio) async fn persist_directory_summaries(
        &self,
        delta: &DirectoryDelta,
    ) -> Result<()> {
        // Workspace/Project TOML is the canonical directory fact; the product SQLite tables remain
        // write-behind mirrors for migration and diagnostics only.
        self.workspaces().apply_delta(delta).await?;
        let catalog = self.catalog();
        for thread in &delta.thread_upserts {
            catalog.upsert(CatalogEntry::from_thread(thread)).await?;
        }
        let archived_at = delta
            .thread_removals
            .iter()
            .map(|removal| removal.archived_at)
            .chain(
                delta
                    .project_removals
                    .iter()
                    .map(|removal| removal.closed_at),
            )
            .max();
        let archived_ids = delta
            .thread_removals
            .iter()
            .flat_map(|removal| removal.thread_ids.iter().cloned())
            .chain(
                delta
                    .project_removals
                    .iter()
                    .flat_map(|removal| removal.thread_ids.iter().cloned()),
            )
            .collect::<Vec<_>>();
        if let Some(archived_at) = archived_at {
            catalog.archive(&archived_ids, archived_at).await?;
        }
        if !delta.session_activity.is_empty() {
            catalog.touch(&delta.session_activity).await?;
        }
        Ok(())
    }
}

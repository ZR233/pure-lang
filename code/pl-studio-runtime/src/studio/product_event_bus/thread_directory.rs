//! Thread 目录：活动热集合维护、SQLite 冷分页 overlay 与目录事实的命令提交。

use anyhow::Result;
use pl_protocol::Thread;

use crate::studio::merged_page::{HotColdEntry, merge_page_desc};
use crate::studio::store::directory::{
    DirectoryDelta, RegisteredChildThread, ThreadDirectoryCursor,
};
use crate::{
    StudioProductEventEnvelope, StudioProductEventKind, StudioThreadDirectoryData,
    StudioThreadDirectoryDelta, StudioThreadDirectoryPage, StudioThreadDirectoryPageData,
    StudioThreadDirectoryState,
};

use super::ProductEventBus;

/// Thread 目录分页的默认页大小上限。
const THREAD_DIRECTORY_PAGE_LIMIT: usize = 100;

impl HotColdEntry for Thread {
    type Key = (i64, String);

    fn page_key(&self) -> Self::Key {
        (self.updated_at, self.id.clone())
    }

    fn entry_id(&self) -> &str {
        &self.id
    }
}

impl ProductEventBus {
    pub(in crate::studio) async fn record_attachments(
        &self,
        records: Vec<crate::studio::AttachmentRecord>,
    ) -> Result<()> {
        self.store.record_attachments(records).await
    }

    pub async fn read_thread_directory(&self) -> Result<StudioThreadDirectoryState> {
        Ok(StudioThreadDirectoryState {
            state: self.resource(
                &self.revisions.thread,
                StudioThreadDirectoryData {
                    threads: self.sorted_thread_index(),
                },
            ),
        })
    }

    /// 会话列表分页：`catalog.toml` 冷分页 + 活动热集合 overlay。
    ///
    /// 同 ID 热条目覆盖冷条目，cursor 键排重；与 Turn 历史共用
    /// [`merge_page_desc`] 合并核心。热集合条目可能尚未耐久化，冷页查询
    /// 以 `limit + 1` 判定 has_more。
    pub async fn read_thread_directory_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<StudioThreadDirectoryPage> {
        let limit = limit.clamp(1, THREAD_DIRECTORY_PAGE_LIMIT);
        let decoded = cursor.and_then(ThreadDirectoryCursor::decode);
        let cursor_key = decoded
            .as_ref()
            .map(|cursor| (cursor.updated_at, cursor.id.clone()));
        let cold = self
            .store
            .list_thread_directory_page(decoded.as_ref(), limit.saturating_add(1))
            .await?;
        let has_more = cold.len() > limit;
        let hot = self
            .thread_index
            .lock()
            .expect("thread index lock poisoned")
            .values()
            .filter(|thread| thread.parent_thread_id.is_none())
            .cloned()
            .collect::<Vec<_>>();
        let mut merged = merge_page_desc(hot, cold, cursor_key.as_ref());
        let has_more = has_more || merged.len() > limit;
        merged.retain(|thread| !thread.archived);
        merged.truncate(limit);
        let next_cursor = has_more
            .then(|| {
                merged.last().map(|thread| ThreadDirectoryCursor {
                    updated_at: thread.updated_at,
                    id: thread.id.clone(),
                })
            })
            .flatten()
            .map(|cursor| cursor.encode());
        Ok(StudioThreadDirectoryPage {
            state: self.resource(
                &self.revisions.thread,
                StudioThreadDirectoryPageData {
                    threads: merged,
                    next_cursor,
                },
            ),
        })
    }

    /// Searches cold history with canonical hot overrides before pagination.
    pub async fn query_threads(
        &self,
        query: &pl_protocol::studio::ThreadDirectoryQuery,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<StudioThreadDirectoryPage> {
        let limit = limit.clamp(1, THREAD_DIRECTORY_PAGE_LIMIT);
        let decoded = cursor
            .map(|value| {
                ThreadDirectoryCursor::decode(value)
                    .ok_or_else(|| anyhow::anyhow!("invalid directory cursor"))
            })
            .transpose()?;
        let key = decoded.as_ref().map(|c| (c.updated_at, c.id.clone()));
        let projects = self.project_snapshot().await;
        let matching_projects = projects
            .iter()
            .filter(|project| {
                query.search.as_ref().is_some_and(|search| {
                    format!("{} {}", project.name, project.path)
                        .to_lowercase()
                        .contains(&search.trim().to_lowercase())
                })
            })
            .map(|project| project.id.clone())
            .collect::<std::collections::HashSet<_>>();
        // Capture the owner before cold reads: even a hot row outside this cursor
        // shadows its older durable version.
        let (hot, revision, updated_at) = {
            let index = self
                .thread_index
                .lock()
                .expect("thread index lock poisoned");
            let (revision, updated_at) = self.revision(&self.revisions.thread);
            (index.clone(), revision, updated_at)
        };
        let mut matches = hot
            .values()
            .filter(|thread| {
                key.as_ref().is_none_or(|key| thread.page_key() < *key)
                    && query.matches(thread, matching_projects.contains(&thread.project_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        matches.sort_by_key(|thread| std::cmp::Reverse(thread.page_key()));
        matches.truncate(limit + 1);
        let mut cold_cursor = decoded;
        loop {
            // Cold search reads only `catalog.toml`; session `state.toml`/`history.sqlite` are never
            // touched (design/17 §17.2).
            let cold = self
                .store
                .catalog_query(query, &matching_projects, cold_cursor.as_ref(), 100)
                .await?;
            let end = cold.last().map(|thread| ThreadDirectoryCursor {
                updated_at: thread.updated_at,
                id: thread.id.clone(),
            });
            let exhausted = cold.len() < 100;
            for thread in cold {
                if !hot.contains_key(&thread.id)
                    && query.matches(&thread, matching_projects.contains(&thread.project_id))
                {
                    matches.push(thread);
                }
            }
            matches.sort_by_key(|thread| std::cmp::Reverse(thread.page_key()));
            matches.truncate(limit + 1);
            let enough = matches.len() > limit
                && end.as_ref().is_some_and(|end| {
                    (end.updated_at, end.id.clone()) <= matches[limit].page_key()
                });
            if exhausted || enough {
                break;
            }
            cold_cursor = end;
        }
        let has_more = matches.len() > limit;
        matches.truncate(limit);
        let next_cursor = has_more
            .then(|| {
                matches.last().map(|thread| {
                    ThreadDirectoryCursor {
                        updated_at: thread.updated_at,
                        id: thread.id.clone(),
                    }
                    .encode()
                })
            })
            .flatten();
        if self.revision(&self.revisions.thread).0 != revision {
            return Err(pl_protocol::studio::StudioError::new(
                pl_protocol::studio::StudioErrorCode::StaleRevision,
                "Directory changed during search; retry the query",
                true,
            )
            .into());
        }
        Ok(StudioThreadDirectoryPage {
            state: pl_protocol::ObservedResource::ready(
                revision,
                updated_at,
                StudioThreadDirectoryPageData {
                    threads: matches,
                    next_cursor,
                },
            ),
        })
    }

    /// 应用一次目录增量并发布 `ThreadDirectoryChanged` 事件（纯内存维护）。
    pub async fn apply_thread_delta(
        &self,
        upserted: Vec<Thread>,
        removed: Vec<String>,
    ) -> Result<StudioProductEventEnvelope> {
        self.publish_thread_delta(upserted, removed, Vec::new())
    }

    fn publish_thread_delta(
        &self,
        upserted: Vec<Thread>,
        removed: Vec<String>,
        archived: Vec<Thread>,
    ) -> Result<StudioProductEventEnvelope> {
        {
            let mut index = self
                .thread_index
                .lock()
                .expect("thread index lock poisoned");
            for thread in &upserted {
                index.insert(thread.id.clone(), thread.clone());
            }
            for id in &removed {
                index.remove(id);
            }
            for thread in archived {
                index.insert(thread.id.clone(), thread);
            }
            self.bump(&self.revisions.thread);
        }
        // State and tombstones are visible before observers receive removal.
        let (revision, updated_at) = self.revision(&self.revisions.thread);
        Ok(self.emit(StudioProductEventKind::ThreadDirectoryChanged(
            StudioThreadDirectoryDelta {
                revision,
                updated_at,
                upserted,
                removed,
            },
        )))
    }

    /// Applies only runtime-owned fields to the current hot directory entry.
    /// Missing or archived entries are never reinserted by a delayed observation.
    pub(in crate::studio) fn patch_thread_runtime(
        &self,
        thread_id: &str,
        status: pl_protocol::ThreadStatus,
        committed_at: i64,
    ) -> Option<Thread> {
        let thread = {
            let mut index = self
                .thread_index
                .lock()
                .expect("thread index lock poisoned");
            let thread = index.get_mut(thread_id)?;
            if thread.archived {
                return None;
            }
            thread.status = status;
            thread.updated_at = thread.updated_at.max(committed_at);
            thread.clone()
        };
        // 状态观察是目录事实：只把有界摘要并入 write-behind 目录队列，不在观察热路径做文件 I/O。
        // 同 Thread 的连续 delta 由队列合并；write-behind 批次经 `apply_directory_delta` 把摘要
        // 幂等写入 catalog/workspaces TOML。调用方在发布后用固定目标 ticket（`flush_through`）
        // 建立保存屏障；状态错误不回滚已提交的内存目录事实。
        self.writer.record_directory(DirectoryDelta {
            thread_upserts: vec![thread.clone()],
            ..Default::default()
        });
        self.bump(&self.revisions.thread);
        let (revision, updated_at) = self.revision(&self.revisions.thread);
        self.emit(StudioProductEventKind::ThreadDirectoryChanged(
            StudioThreadDirectoryDelta {
                revision,
                updated_at,
                upserted: vec![thread.clone()],
                removed: Vec::new(),
            },
        ));
        Some(thread)
    }

    /// 将已从持久化层加载的目录条目加入热集合，但不改变 revision 或广播事件。
    ///
    /// 激活路径只是建立查询缓存，不代表目录事实发生了变化；真正的目录
    /// mutation 必须继续通过 [`Self::apply_thread_delta`] 提交。
    pub(crate) fn warm_thread_index(&self, entries: Vec<Thread>) {
        if entries.is_empty() {
            return;
        }
        let mut index = self
            .thread_index
            .lock()
            .expect("thread index lock poisoned");
        for thread in entries {
            index.entry(thread.id.clone()).or_insert(thread);
        }
    }

    /// 提交一次目录事实：在内存登记事实、更新热集合并广播。
    ///
    /// 这是 Thread/Project 目录 mutation 的唯一命令通道；后台保存不能拒绝目录事实。
    pub(in crate::studio) async fn commit_directory(
        &self,
        delta: DirectoryDelta,
    ) -> Result<StudioProductEventEnvelope> {
        if delta.is_empty() {
            return Err(anyhow::anyhow!("directory delta is empty"));
        }
        // Durable first-screen fact first: `catalog.toml` is the lightweight session summary that
        // startup/pagination/search consume. The write-behind queue mirrors the same delta into the
        // other fact sources (design/17 §17.3).
        self.store.persist_directory_summaries(&delta).await?;
        let mut archived = Vec::new();
        {
            let mut index = self
                .thread_index
                .lock()
                .expect("thread index lock poisoned");
            if self.writer.pending_commit_count() == 0 {
                index.retain(|_, thread| !thread.archived);
            }
            let removals = delta
                .thread_removals
                .iter()
                .flat_map(|removal| {
                    removal
                        .thread_ids
                        .iter()
                        .map(move |id| (id, removal.archived_at))
                })
                .chain(delta.project_removals.iter().flat_map(|removal| {
                    removal
                        .thread_ids
                        .iter()
                        .map(move |id| (id, removal.closed_at))
                }));
            for (id, at) in removals {
                if let Some(thread) = index.get(id) {
                    let mut thread = thread.clone();
                    thread.archived = true;
                    thread.updated_at = at;
                    archived.push(thread);
                }
            }
        }
        self.writer.record_directory(delta.clone());
        let (thread_upserts, thread_removals): (Vec<Thread>, Vec<String>) = (
            delta.thread_upserts.clone(),
            delta
                .thread_removals
                .iter()
                .flat_map(|removal| removal.thread_ids.iter().cloned())
                .chain(
                    delta
                        .project_removals
                        .iter()
                        .flat_map(|removal| removal.thread_ids.iter().cloned()),
                )
                .collect(),
        );
        let mut envelope = None;
        if !thread_upserts.is_empty() || !thread_removals.is_empty() {
            envelope =
                Some(self.publish_thread_delta(thread_upserts, thread_removals, archived)?);
        }
        let project_upserts = delta
            .project_upserts
            .iter()
            .map(|project| crate::ProjectRecord {
                id: project.id.clone(),
                name: project.name.clone(),
                path: project.path.clone(),
                ssh_alias: project.ssh_alias.clone(),
                updated_at: project.updated_at,
            })
            .collect::<Vec<_>>();
        if let Some(project_event) = self
            .apply_project_delta(&project_upserts, &delta.project_removals)
            .await?
        {
            envelope = Some(project_event);
        }
        envelope.ok_or_else(|| anyhow::anyhow!("directory delta has no observable changes"))
    }

    fn sorted_thread_index(&self) -> Vec<Thread> {
        let mut threads = self
            .thread_index
            .lock()
            .expect("thread index lock poisoned")
            .values()
            .filter(|thread| !thread.archived && thread.parent_thread_id.is_none())
            .cloned()
            .collect::<Vec<_>>();
        threads.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        threads
    }

    /// 从活动热集合读取 Thread 元数据；纯冷数据请走分页冷查询。
    pub(crate) fn thread_snapshot(&self, thread_id: &str) -> Option<Thread> {
        self.thread_index
            .lock()
            .expect("thread index lock poisoned")
            .get(thread_id)
            .cloned()
    }

    /// 注册一个 child Thread：typed delta 与热集合共同形成内存提交。
    pub(in crate::studio) async fn register_child_thread(
        &self,
        spec: RegisteredChildThread,
    ) -> Result<()> {
        let delta = DirectoryDelta::register_child_thread(spec);
        self.commit_directory(delta).await?;
        Ok(())
    }

    pub(in crate::studio) async fn fault_unregistered_child(
        &self,
        id: &str,
        message: &str,
    ) -> Result<()> {
        let mut thread = self
            .thread_snapshot(id)
            .ok_or_else(|| anyhow::anyhow!("unregistered child is not resident: {id}"))?;
        let state = crate::studio::records::DirectoryState {
            kind: pl_protocol::ThreadStatus::Faulted,
            error: Some(message.into()),
        };
        thread.status = pl_protocol::ThreadStatus::Faulted;
        thread.updated_at = crate::studio::unix_seconds();
        let delta = DirectoryDelta {
            unregistered_faults: vec![crate::studio::store::directory::UnregisteredChildFault {
                thread_id: id.into(),
                state,
            }],
            thread_upserts: vec![thread],
            ..Default::default()
        };
        self.commit_directory(delta).await?;
        Ok(())
    }

    /// 热集合移除一个已耐久化且不再活动的 Thread 条目（LRU 淘汰路径）。
    pub(in crate::studio) fn evict_thread_entry(&self, thread_id: &str) {
        self.thread_index
            .lock()
            .expect("thread index lock poisoned")
            .remove(thread_id);
    }

    /// 热集合中属于指定 root 的全部条目（树归档时叠加尚未落库的 child）。
    pub(in crate::studio) fn threads_for_root(&self, root_thread_id: &str) -> Vec<Thread> {
        self.thread_index
            .lock()
            .expect("thread index lock poisoned")
            .values()
            .filter(|thread| thread.root_thread_id == root_thread_id)
            .cloned()
            .collect()
    }
}

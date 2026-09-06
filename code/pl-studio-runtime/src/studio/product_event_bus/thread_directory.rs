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
    pub(in crate::studio) fn record_attachments(
        &self,
        records: Vec<crate::studio::AttachmentRecord>,
    ) {
        self.writer.record_attachments(records);
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

    /// 会话列表分页：SQLite 冷分页 + 活动热集合 overlay。
    ///
    /// 同 ID 热条目覆盖冷行，cursor 键排重；与 Turn 历史共用
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
            .cloned()
            .collect::<Vec<_>>();
        let mut merged = merge_page_desc(hot, cold, cursor_key.as_ref());
        let has_more = has_more || merged.len() > limit;
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

    /// 应用一次目录增量并发布 `ThreadDirectoryChanged` 事件（纯内存维护）。
    pub async fn apply_thread_delta(
        &self,
        upserted: Vec<Thread>,
        removed: Vec<String>,
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
        }
        self.bump(&self.revisions.thread);
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

    /// 将已从持久化层加载的目录条目加入热集合，但不改变 revision 或广播事件。
    ///
    /// 激活路径只是建立查询缓存，不代表目录事实发生了变化；真正的目录
    /// mutation 必须继续通过 [`Self::apply_thread_delta`] 提交。
    pub(in crate::studio) fn warm_thread_index(&self, entries: Vec<Thread>) {
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
            envelope = Some(
                self.apply_thread_delta(thread_upserts, thread_removals)
                    .await?,
            );
        }
        let project_upserts = delta
            .project_upserts
            .iter()
            .map(|project| crate::ProjectRecord {
                id: project.id.clone(),
                name: project.name.clone(),
                path: project.path.clone(),
                ssh_server_id: project.ssh_server_id.clone(),
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
    pub(in crate::studio) fn thread_snapshot(&self, thread_id: &str) -> Option<Thread> {
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
        self.writer.record_directory(delta.clone());
        self.apply_thread_delta(delta.thread_upserts.clone(), Vec::new())
            .await?;
        Ok(())
    }

    pub(in crate::studio) async fn fault_unregistered_child(
        &self,
        id: &str,
        message: &str,
    ) -> Result<()> {
        use pl_core::AgentStateTransition;
        let mut thread = self
            .thread_snapshot(id)
            .ok_or_else(|| anyhow::anyhow!("unregistered child is not resident: {id}"))?;
        let state = pl_core::AgentState::idle()
            .decide(pl_core::AgentCommand::Fault {
                error: pl_protocol::StateError {
                    code: "agentRegistrationFailed".into(),
                    message: message.into(),
                    retryable: false,
                },
                turn_id: None,
                classification: pl_core::AgentFaultClassification::RecoverableRuntime,
            })?
            .next_state;
        thread.status = pl_protocol::ThreadStatus::Faulted;
        thread.updated_at = crate::studio::unix_seconds();
        let delta = DirectoryDelta {
            unregistered_faults: vec![crate::studio::store::directory::UnregisteredChildFault {
                thread_id: id.into(),
                state,
            }],
            ..Default::default()
        };
        self.writer.record_directory(delta);
        self.apply_thread_delta(vec![thread], Vec::new()).await?;
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

#[cfg(test)]
mod tests {
    use pl_protocol::Thread;

    use crate::studio::StudioStore;
    use crate::studio::ids::unix_seconds;

    use super::super::tests::{memory_bus, seed_project};

    async fn seed_cold_threads(store: &StudioStore, project_id: &str, count: i64) {
        for index in 0..count {
            store
                .create_thread(
                    project_id,
                    &format!("Session {index}"),
                    pl_protocol::ThreadModeId::simple(),
                )
                .await
                .expect("thread");
        }
    }

    #[tokio::test]
    async fn thread_directory_page_walks_cold_keyset_cursor_without_overlap() {
        let (store, runtime) = memory_bus().await;
        let project = seed_project(&store).await;
        seed_cold_threads(&store, &project.id, 7).await;

        let first = runtime
            .read_thread_directory_page(None, 3)
            .await
            .expect("first page");
        let first_data = first.state.value().expect("ready first page");
        assert_eq!(first_data.threads.len(), 3);
        let cursor = first_data.next_cursor.clone().expect("first page has more");

        let second = runtime
            .read_thread_directory_page(Some(&cursor), 3)
            .await
            .expect("second page");
        let second_data = second.state.value().expect("ready second page");
        assert_eq!(second_data.threads.len(), 3);
        assert_ne!(
            second_data.threads.first().unwrap().id,
            first_data.threads.last().unwrap().id
        );

        let cursor = second_data
            .next_cursor
            .clone()
            .expect("second page has more");
        let third = runtime
            .read_thread_directory_page(Some(&cursor), 3)
            .await
            .expect("third page");
        let third_data = third.state.value().expect("ready third page");
        assert_eq!(third_data.threads.len(), 1);
        assert!(third_data.next_cursor.is_none());
    }

    #[tokio::test]
    async fn hot_entries_overlay_cold_rows_and_fill_the_pending_window() {
        let (store, runtime) = memory_bus().await;
        let project = seed_project(&store).await;
        seed_cold_threads(&store, &project.id, 3).await;

        // 冷端第二页条目：一个被热事实覆盖，一个保持冷态。
        let cold = store
            .list_thread_directory_page(None, 10)
            .await
            .expect("cold page");
        let oldest = cold.last().expect("oldest cold thread").clone();
        let middle = cold[1].clone();
        let mut hot_overlay = middle.clone();
        hot_overlay.title = "hot refreshed".to_string();
        hot_overlay.updated_at += 100;
        // 尚未落库的新 Thread 只存在于热集合。
        let mut pending = oldest.clone();
        pending.id = format!("{}-new", pending.id);
        pending.agent_path = pending.id.clone();
        pending.updated_at += 200;

        runtime
            .apply_thread_delta(vec![hot_overlay.clone(), pending.clone()], Vec::new())
            .await
            .expect("hot delta");

        let page = runtime
            .read_thread_directory_page(None, 10)
            .await
            .expect("page");
        let threads = page.state.value().expect("ready page").threads.clone();
        assert_eq!(threads.len(), 4);
        // 热覆盖胜出且位于其新 key 位置；未落库条目参与排序。
        assert_eq!(threads.first().unwrap().id, pending.id);
        assert_eq!(threads[1].id, hot_overlay.id);
        assert_eq!(threads[1].title, "hot refreshed");
        // 同 id 冷行被覆盖，不重复出现。
        assert_eq!(threads.iter().filter(|t| t.id == hot_overlay.id).count(), 1);
    }

    #[tokio::test]
    async fn hot_removal_leaves_only_cold_entries_in_pages() {
        let (store, runtime) = memory_bus().await;
        let project = seed_project(&store).await;
        seed_cold_threads(&store, &project.id, 1).await;
        let cold = store
            .list_thread_directory_page(None, 10)
            .await
            .expect("cold page");
        let thread = cold.first().expect("seeded thread").clone();

        // 热集合移除（归档/淘汰）后条目回到纯冷态，仍由冷分页可见。
        runtime
            .apply_thread_delta(Vec::new(), vec![thread.id.clone()])
            .await
            .expect("delta");
        let hot_only = runtime.read_thread_directory().await.expect("hot read");
        assert!(
            hot_only
                .state
                .value()
                .expect("ready directory")
                .threads
                .is_empty()
        );

        let page = runtime
            .read_thread_directory_page(None, 10)
            .await
            .expect("page");
        let page = page.state.value().expect("ready page");
        assert_eq!(page.threads.len(), 1);
        assert_eq!(page.threads.first().unwrap().id, thread.id);
    }

    #[tokio::test]
    async fn warming_thread_metadata_does_not_change_revision_or_emit_event() {
        let (_store, bus) = memory_bus().await;
        let mut events = bus.subscribe();
        let before = bus.read_thread_directory().await.expect("directory");
        let before_revision = before.state.revision();
        let mut entry = Thread::placeholder("warm-only");
        entry.agent_path = entry.id.clone();
        entry.project_id = "project".to_string();
        entry.title = "Warm only".to_string();
        entry.updated_at = unix_seconds();

        bus.warm_thread_index(vec![entry.clone()]);

        let mut stale = entry.clone();
        stale.title = "Stale cold title".to_string();
        bus.warm_thread_index(vec![stale]);
        let after = bus.read_thread_directory().await.expect("directory");
        assert_eq!(after.state.revision(), before_revision);
        assert_eq!(after.state.value().unwrap().threads, vec![entry]);
        assert!(events.try_recv().is_err());
    }
}

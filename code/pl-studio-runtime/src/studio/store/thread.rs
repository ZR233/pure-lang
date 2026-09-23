//! Thread 目录记录的冷读取入口。
//!
//! 目录 mutation 已统一走 `store::directory::DirectoryDelta` 的 write-behind
//! 通道（design/17 §17.2）；本文件只保留命令路径允许的聚合冷加载与分页查询。

use anyhow::Result;

use crate::studio::catalog::CatalogEntry;
use crate::studio::records::ThreadRecord;
use crate::studio::store::StudioStore;

impl StudioStore {
    pub async fn list_root_threads(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        Ok(self
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| {
                entry.project_id == project_id
                    && !entry.archived
                    && entry.parent_thread_id.is_none()
            })
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    /// Archive selection reads only directory facts, not historical journals.
    pub async fn list_root_threads_for_archive(
        &self,
        root_thread_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        let Some(root) = self.catalog().get(root_thread_id) else {
            return Ok(Vec::new());
        };
        Ok(self
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| {
                entry.project_id == root.project_id
                    && !entry.archived
                    && entry.parent_thread_id.is_none()
            })
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    /// Archive scope includes every active descendant without replaying its journal.
    pub async fn list_threads_for_archive(
        &self,
        root_thread_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        Ok(catalog_tree(self, root_thread_id, false)
            .into_iter()
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    /// Active descendants of one root. The catalog already carries the persisted status summary.
    pub async fn list_threads_for_root(&self, root_thread_id: &str) -> Result<Vec<ThreadRecord>> {
        self.list_threads_for_archive(root_thread_id).await
    }

    /// Cold baseline for explicit archive restoration, including archived descendants.
    pub(in crate::studio) async fn read_directory_tree(
        &self,
        root_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        Ok(catalog_tree(self, root_id, true)
            .into_iter()
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    /// Project 归档 activation 一次性装载其完整 Thread 目录。
    pub async fn list_threads_for_project(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        let mut entries = self
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| entry.project_id == project_id)
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(entries
            .into_iter()
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    pub async fn list_project_thread_ids(&self, project_id: &str) -> Result<Vec<String>> {
        Ok(self
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| entry.project_id == project_id)
            .map(|entry| entry.id)
            .collect())
    }

    pub async fn read_thread(&self, thread_id: &str) -> Result<Option<ThreadRecord>> {
        Ok(self
            .catalog()
            .get(thread_id)
            .map(|entry| thread_record_from_catalog(&entry)))
    }

    pub(in crate::studio) async fn read_thread_association(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadRecord>> {
        self.read_thread(thread_id).await
    }
}

fn thread_record_from_catalog(entry: &CatalogEntry) -> ThreadRecord {
    ThreadRecord::from_directory_thread(entry.to_thread())
}

fn catalog_tree(store: &StudioStore, root_id: &str, include_archived: bool) -> Vec<CatalogEntry> {
    let mut entries = store
        .catalog()
        .entries()
        .into_iter()
        .filter(|entry| entry.root_thread_id == root_id && (include_archived || !entry.archived))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    entries
}

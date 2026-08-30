use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use pl_core::ThreadId;
use tokio::sync::RwLock;

use crate::AttachmentRecord;

#[derive(Clone)]
pub(super) struct StudioAgentResource {
    pub(super) thread_id: String,
    pub(super) assignment_name: String,
}

#[derive(Clone, Default)]
pub(in crate::studio) struct StudioAgentResources {
    entries: Arc<RwLock<BTreeMap<ThreadId, StudioAgentResource>>>,
    tool_sets: Arc<RwLock<BTreeMap<ThreadId, pl_core::AgentToolSet>>>,
    initial_remote_urls: Arc<RwLock<BTreeMap<String, String>>>,
    attachments: Arc<RwLock<BTreeMap<String, BTreeMap<String, AttachmentRecord>>>>,
}

impl StudioAgentResources {
    pub(super) async fn insert(&self, id: ThreadId, resource: StudioAgentResource) {
        // 新 child Thread 不继承父 Thread 的附件；即使集合为空，也必须先建立
        // catalog 驻留标记，确保 activate_spawn 返回后首个 Turn 只读热对象。
        self.attachments
            .write()
            .await
            .entry(resource.thread_id.clone())
            .or_default();
        self.entries.write().await.insert(id, resource);
    }

    pub(super) async fn get(&self, id: &ThreadId) -> Option<StudioAgentResource> {
        self.entries.read().await.get(id).cloned()
    }

    pub(super) async fn remove(&self, id: &ThreadId) -> Option<StudioAgentResource> {
        self.tool_sets.write().await.remove(id);
        let resource = self.entries.write().await.remove(id);
        if let Some(resource) = &resource {
            self.attachments.write().await.remove(&resource.thread_id);
        }
        resource
    }

    pub(super) async fn tool_set(
        &self,
        id: &ThreadId,
        manager: &pl_core::ToolManager,
    ) -> pl_core::AgentToolSet {
        let mut sets = self.tool_sets.write().await;
        sets.entry(id.clone())
            .or_insert_with(|| {
                manager.agent_tool_set(id.to_string(), pl_core::GlobalToolInheritance::Isolated)
            })
            .clone()
    }

    pub(super) async fn release_after_close(&self, id: &ThreadId) {
        self.remove(id).await;
    }

    pub(super) async fn thread_id(&self, id: &ThreadId) -> Option<String> {
        Some(id.to_string())
    }

    pub(in crate::studio) async fn insert_initial_remote_urls(
        &self,
        urls: impl IntoIterator<Item = (String, String)>,
    ) {
        self.initial_remote_urls.write().await.extend(urls);
    }

    pub(super) async fn take_initial_remote_urls(
        &self,
        attachment_ids: &[String],
    ) -> BTreeMap<String, String> {
        let mut urls = self.initial_remote_urls.write().await;
        attachment_ids
            .iter()
            .filter_map(|attachment_id| {
                urls.remove(attachment_id)
                    .map(|url| (attachment_id.clone(), url))
            })
            .collect()
    }

    pub(in crate::studio) async fn remove_initial_remote_urls(&self, attachment_ids: &[String]) {
        let mut urls = self.initial_remote_urls.write().await;
        for attachment_id in attachment_ids {
            urls.remove(attachment_id);
        }
    }

    pub(in crate::studio) async fn replace_thread_attachments(
        &self,
        thread_id: &str,
        records: Vec<AttachmentRecord>,
    ) {
        self.attachments.write().await.insert(
            thread_id.to_string(),
            records
                .into_iter()
                .map(|record| (record.id.clone(), record))
                .collect(),
        );
    }

    pub(in crate::studio) async fn insert_thread_attachments(
        &self,
        thread_id: &str,
        records: impl IntoIterator<Item = AttachmentRecord>,
    ) {
        let mut attachments = self.attachments.write().await;
        let catalog = attachments.entry(thread_id.to_string()).or_default();
        catalog.extend(
            records
                .into_iter()
                .map(|record| (record.id.clone(), record)),
        );
    }

    pub(in crate::studio) async fn thread_attachments(
        &self,
        thread_id: &str,
    ) -> Vec<AttachmentRecord> {
        self.attachments
            .read()
            .await
            .get(thread_id)
            .map(|catalog| catalog.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(in crate::studio) async fn selected_thread_attachments(
        &self,
        thread_id: &str,
        attachment_ids: &[String],
    ) -> anyhow::Result<Vec<AttachmentRecord>> {
        let attachments = self.attachments.read().await;
        let catalog = attachments
            .get(thread_id)
            .ok_or_else(|| anyhow::anyhow!("Thread attachment catalog is not resident"))?;
        let mut selected = Vec::with_capacity(attachment_ids.len());
        let mut seen = BTreeSet::new();
        for id in attachment_ids {
            anyhow::ensure!(seen.insert(id), "duplicate attachment id: {id}");
            selected.push(
                catalog
                    .get(id)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("attachment {id} is not resident"))?,
            );
        }
        Ok(selected)
    }

    pub(in crate::studio) async fn remove_thread_attachment_ids(
        &self,
        thread_id: &str,
        attachment_ids: &[String],
    ) {
        let mut attachments = self.attachments.write().await;
        if let Some(catalog) = attachments.get_mut(thread_id) {
            for id in attachment_ids {
                catalog.remove(id);
            }
        }
    }

    pub(in crate::studio) async fn evict_thread_attachments(&self, thread_id: &str) {
        self.attachments.write().await.remove(thread_id);
    }
}

pub(in crate::studio) fn root_agent_id(thread_id: &str) -> ThreadId {
    ThreadId::new(thread_id).expect("Studio Thread id 必须非空")
}

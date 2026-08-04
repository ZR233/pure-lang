use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use tokio::sync::{Mutex, broadcast};

use crate::{StudioAgentDirectoryEntry, StudioProductEventEnvelope, StudioProductEventKind};

use super::{StudioStore, ids::unix_seconds};

/// Studio 低频产品事件通道。
///
/// 该通道不承载任何 Thread timeline 状态。消费者在 lag 后重新加载产品快照；选中
/// Thread 的高频状态通过 Thread subscription 的 authoritative snapshot 恢复。
#[derive(Clone)]
pub struct StudioProductEventRuntime {
    store: StudioStore,
    tx: broadcast::Sender<StudioProductEventEnvelope>,
    sequence: Arc<AtomicU64>,
    task_snapshots: Arc<Mutex<BTreeMap<String, Option<crate::StudioTaskRuntime>>>>,
}

impl StudioProductEventRuntime {
    pub fn new(store: StudioStore) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            store,
            tx,
            sequence: Arc::new(AtomicU64::new(0)),
            task_snapshots: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StudioProductEventEnvelope> {
        self.tx.subscribe()
    }

    pub fn emit(
        &self,
        project_id: Option<String>,
        kind: StudioProductEventKind,
    ) -> StudioProductEventEnvelope {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let envelope = StudioProductEventEnvelope {
            event_id: format!("studio-product-{sequence}"),
            project_id,
            sequence,
            created_at: unix_seconds(),
            kind,
        };
        let _ = self.tx.send(envelope.clone());
        envelope
    }

    pub async fn emit_thread_directory(
        &self,
        project_id: &str,
    ) -> Result<StudioProductEventEnvelope> {
        let threads = self
            .store
            .list_threads(project_id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(self.emit(
            Some(project_id.to_string()),
            StudioProductEventKind::ThreadDirectoryChanged {
                project_id: project_id.to_string(),
                threads,
            },
        ))
    }

    pub fn emit_agent_directory(
        &self,
        project_id: &str,
        agent: StudioAgentDirectoryEntry,
    ) -> StudioProductEventEnvelope {
        let root_thread_id = agent.root_thread_id.clone();
        self.emit(
            Some(project_id.to_string()),
            StudioProductEventKind::AgentDirectoryChanged {
                root_thread_id,
                agent: Box::new(agent),
            },
        )
    }

    /// 重新读取并在内容真正变化时发布 Studio task 快照。
    pub async fn refresh_task(
        &self,
        root_thread_id: &str,
    ) -> Result<Option<StudioProductEventEnvelope>> {
        let task = super::task_projection::load_task_runtime(&self.store, root_thread_id).await?;
        let mut snapshots = self.task_snapshots.lock().await;
        if snapshots.get(root_thread_id) == Some(&task)
            || (!snapshots.contains_key(root_thread_id) && task.is_none())
        {
            return Ok(None);
        }
        snapshots.insert(root_thread_id.to_string(), task.clone());
        drop(snapshots);
        Ok(Some(self.emit(
            None,
            StudioProductEventKind::TaskChanged {
                root_thread_id: root_thread_id.to_string(),
                task: task.map(Box::new),
            },
        )))
    }
}

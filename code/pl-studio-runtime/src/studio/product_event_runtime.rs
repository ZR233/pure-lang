use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use tokio::sync::{Mutex, broadcast};

use crate::{StudioProductEventEnvelope, StudioProductEventKind, StudioSessionSummary};

use super::{StudioStore, ids::unix_seconds};

/// Studio 低频产品事件通道。
///
/// 该通道不承载任何 session timeline 状态。消费者在 lag 后重新加载产品快照；会话
/// 内状态始终通过 PL 的 `SessionEventHub` 恢复。
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

    pub async fn emit_session_list(&self, project_id: &str) -> Result<StudioProductEventEnvelope> {
        let sessions = self
            .store
            .list_sessions(project_id)
            .await?
            .into_iter()
            .map(|session| StudioSessionSummary {
                id: session.id,
                project_id: session.project_id,
                title: session.title,
                mode: session.mode,
                updated_at: session.updated_at,
                visibility: session.visibility.as_str().to_string(),
                parent_session_id: session.parent_session_id,
            })
            .collect();
        Ok(self.emit(
            Some(project_id.to_string()),
            StudioProductEventKind::SessionListChanged {
                project_id: project_id.to_string(),
                sessions,
            },
        ))
    }

    /// 重新读取并在内容真正变化时发布 Studio task 快照。
    pub async fn refresh_session_task(
        &self,
        session_id: &str,
    ) -> Result<Option<StudioProductEventEnvelope>> {
        let task = super::task_projection::load_task_runtime(&self.store, session_id).await?;
        let mut snapshots = self.task_snapshots.lock().await;
        if snapshots.get(session_id) == Some(&task)
            || (!snapshots.contains_key(session_id) && task.is_none())
        {
            return Ok(None);
        }
        snapshots.insert(session_id.to_string(), task.clone());
        drop(snapshots);
        Ok(Some(self.emit(
            None,
            StudioProductEventKind::SessionTaskChanged {
                session_id: session_id.to_string(),
                task,
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::StudioMode;

    #[tokio::test]
    async fn product_channel_only_publishes_global_state() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("/tmp/product-events").await.unwrap();
        let session = store
            .create_session(&project.id, "会话", StudioMode::Simple)
            .await
            .unwrap();
        let runtime = StudioProductEventRuntime::new(store);
        let mut receiver = runtime.subscribe();

        let emitted = runtime.emit_session_list(&project.id).await.unwrap();
        let received = receiver.recv().await.unwrap();

        assert_eq!(received, emitted);
        assert_eq!(received.sequence, 1);
        assert_eq!(
            received.kind,
            StudioProductEventKind::SessionListChanged {
                project_id: project.id,
                sessions: vec![StudioSessionSummary {
                    id: session.id,
                    project_id: session.project_id,
                    title: session.title,
                    mode: session.mode,
                    updated_at: session.updated_at,
                    visibility: session.visibility.as_str().to_string(),
                    parent_session_id: session.parent_session_id,
                }],
            }
        );
    }
}

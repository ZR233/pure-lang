//! Thread 对应 agent 的激活、驻留恢复与 canonical owner 读取。

use anyhow::{Context, Result};

use crate::studio::{ThreadKind, ThreadRecord, ThreadVisibility};

use super::super::StudioRuntime;

impl StudioRuntime {
    pub(in crate::studio::runtime) async fn register_new_thread(
        &self,
        thread: ThreadRecord,
    ) -> Result<()> {
        self.ensure_thread_owner(&thread.id).await?;
        Ok(())
    }

    pub(in crate::studio::runtime) async fn read_owned_thread(
        &self,
        thread_id: &str,
    ) -> Result<ThreadRecord> {
        let thread = if let Some(thread) = self
            .agent_facility
            .product_events
            .thread_snapshot(thread_id)
        {
            thread
        } else {
            self.store
                .read_thread(thread_id)
                .await?
                .map(pl_protocol::Thread::from)
                .context("selected Thread not found")?
        };
        if self.recovery_issues().iter().any(|issue| {
            issue.scope == crate::StudioRecoveryIssueScope::Thread
                && issue.thread_id.as_deref() == Some(thread.root_thread_id.as_str())
        }) {
            return Err(anyhow::Error::new(pl_protocol::studio::StudioError::new(
                pl_protocol::studio::StudioErrorCode::Protocol,
                "This Thread is blocked because its durable timeline is incompatible; use the recovery cleanup action",
                false,
            )));
        }
        Ok(ThreadRecord {
            id: thread.id,
            project_id: thread.project_id,
            title: thread.title,
            mode: thread.mode,
            created_at: thread.created_at,
            updated_at: thread.updated_at,
            visibility: if thread.archived {
                ThreadVisibility::Archived
            } else {
                ThreadVisibility::Active
            },
            thread_kind: if thread.parent_thread_id.is_some() {
                ThreadKind::Agent
            } else {
                ThreadKind::Root
            },
            parent_thread_id: thread.parent_thread_id,
            root_thread_id: thread.root_thread_id,
            agent_path: thread.agent_path,
            role: thread.role,
            status: thread.status,
            summary: None,
            error: None,
            runtime_updated_at: None,
        })
    }
}

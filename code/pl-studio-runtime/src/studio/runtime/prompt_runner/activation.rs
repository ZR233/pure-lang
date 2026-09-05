//! Thread 对应 agent 的激活、驻留恢复与 canonical owner 读取。

use anyhow::{Context, Result};

use crate::config::StudioRole;
use crate::studio::agent_host::root_agent_id;
use crate::studio::{ThreadKind, ThreadRecord, ThreadVisibility};
use pl_core::ThreadRepository as _;

use super::super::StudioRuntime;

impl StudioRuntime {
    pub(in crate::studio) async fn ensure_thread_agent(
        &self,
        thread_id: &str,
    ) -> Result<(pl_core::AgentRuntimeHandle, pl_core::ThreadId)> {
        let target = self.read_owned_thread(thread_id).await?;
        self.ensure_thread_agent_for_record(target).await
    }

    pub(super) async fn ensure_thread_agent_for_record(
        &self,
        target: ThreadRecord,
    ) -> Result<(pl_core::AgentRuntimeHandle, pl_core::ThreadId)> {
        let framework = self.agent_framework().await?;
        let handle = framework.handle();
        let target_agent_id = pl_core::ThreadId::new(target.agent_path.clone())?;
        let target_thread_id = target.id.clone();
        let mut missing = Vec::new();
        let mut current = target.clone();
        loop {
            let agent_path = pl_core::ThreadId::new(current.agent_path.clone())?;
            match handle.snapshot(agent_path).await {
                Ok(_) => break,
                Err(pl_core::AgentRuntimeError::NotFound(_)) => {}
                Err(error) => return Err(anyhow::anyhow!(error)),
            }
            let parent_thread_id = current.parent_thread_id.clone();
            missing.push(current);
            let Some(parent_thread_id) = parent_thread_id else {
                break;
            };
            current = self.read_owned_thread(&parent_thread_id).await?;
        }

        for thread_record in missing.into_iter().rev() {
            self.ensure_thread_resident(&handle, thread_record).await?;
        }
        handle
            .snapshot(target_agent_id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        self.residency.touch(&target_thread_id).await;
        // 激活即入热集合：目录分页在此之后能以内存事实覆盖冷行。
        self.agent_facility
            .product_events
            .warm_thread_index(vec![pl_protocol::Thread::from(target)]);
        self.enforce_residency_limit().await;
        Ok((handle, target_agent_id))
    }

    /// 让单个缺失的 Thread 驻留：已注册过 runtime 的走 durable 恢复，
    /// 从未注册的沿用初始注册（空 session）。
    async fn ensure_thread_resident(
        &self,
        handle: &pl_core::AgentRuntimeHandle,
        thread_record: ThreadRecord,
    ) -> Result<()> {
        let attachments = self
            .store
            .list_thread_attachments(&thread_record.id)
            .await?;
        let registered = self
            .store
            .read_thread_runtime_revision(&thread_record.id)
            .await?
            > 0;
        if registered {
            // 共享 writer 的 repository 实例：恢复基线 seed 进进程级 writer，
            // 不构造即弃的第二 writer（design/17 §17.2）。
            let repository = self
                .persistence_repository()
                .await
                .context("Studio persistence writer is unavailable")?;
            let thread_id = pl_core::ThreadId::new(thread_record.id.clone())?;
            let Some(restored) = repository.restore_thread(&thread_id).await? else {
                anyhow::bail!(
                    "Thread {} has a corrupt durable session and cannot be activated",
                    thread_record.id
                );
            };
            match handle.restore_agent(restored).await {
                Ok(_) | Err(pl_core::AgentRuntimeError::AlreadyExists(_)) => {}
                Err(error) => return Err(anyhow::anyhow!(error)),
            }
            self.agent_facility
                .resources
                .replace_thread_attachments(&thread_record.id, attachments)
                .await;
            self.residency.touch(&thread_record.id).await;
            return Ok(());
        }
        let registration = self
            .thread_agent_registration(handle, thread_record.clone())
            .await?;
        match handle.register(registration).await {
            Ok(_) | Err(pl_core::AgentRuntimeError::AlreadyExists(_)) => {}
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
        self.agent_facility
            .resources
            .replace_thread_attachments(&thread_record.id, attachments)
            .await;
        self.residency.touch(&thread_record.id).await;
        Ok(())
    }

    async fn thread_agent_registration(
        &self,
        handle: &pl_core::AgentRuntimeHandle,
        thread_record: ThreadRecord,
    ) -> Result<pl_core::AgentRegistration> {
        let agent_id = pl_core::ThreadId::new(thread_record.agent_path.clone())?;
        let (parent_id, role, depth) = match thread_record.thread_kind {
            ThreadKind::Root => {
                anyhow::ensure!(
                    thread_record.parent_thread_id.is_none()
                        && agent_id == root_agent_id(&thread_record.id),
                    "root Studio Thread {} has invalid canonical owner",
                    thread_record.id
                );
                let role = StudioRole::Planner;
                (None, role, 0)
            }
            ThreadKind::Agent => {
                anyhow::ensure!(
                    agent_id != root_agent_id(&thread_record.root_thread_id),
                    "child Studio Thread {} cannot use a root agent identity",
                    thread_record.id
                );
                let parent_thread_id = thread_record
                    .parent_thread_id
                    .as_deref()
                    .context("child Studio Thread has no parent Thread")?;
                let parent = self.read_owned_thread(parent_thread_id).await?;
                let parent_id = pl_core::ThreadId::new(parent.agent_path)?;
                let parent_snapshot = handle
                    .snapshot(parent_id.clone())
                    .await
                    .map_err(|error| anyhow::anyhow!(error))?;
                let role = StudioRole::from_key(&thread_record.role)
                    .context("child Studio Thread has an unsupported owner role")?;
                (Some(parent_id), role, parent_snapshot.identity.depth + 1)
            }
        };
        // 新建 Thread 已先进入 typed write-behind 与热目录，首次 actor 注册不得
        // 为等待 SQLite 再序列化/回读。冷激活则使用 durable seed。
        let seed = self
            .store
            .thread_runtime_seed(&thread_record.id)
            .await?
            .unwrap_or(crate::studio::store::ThreadRuntimeSeed {
                thread_revision: 0,
                runtime_revision: 1,
                event_sequence: 1,
            });
        let registration = pl_core::AgentRegistration {
            identity: pl_core::AgentIdentity {
                id: agent_id,
                parent_id,
                role: role.id(),
                depth,
            },
            session: pl_core::ThreadContextState {
                metadata: pl_core::ThreadContextMetadata {
                    project_id: Some(thread_record.project_id),
                    title: Some(thread_record.title),
                },
                session: pl_core::AgentSession::new(),
                usage: pl_protocol::TokenUsage::default(),
                billing_by_turn: std::collections::BTreeMap::new(),
                last_context_tokens: None,
                trace_sequence: 0,
                thread_revision: seed.thread_revision,
            },
            runtime_revision: seed.runtime_revision,
            event_sequence: seed.event_sequence,
        };
        Ok(registration)
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

    pub(super) async fn thread_agent_path(&self, thread_id: &str) -> Result<pl_core::ThreadId> {
        let thread = self.read_owned_thread(thread_id).await?;
        pl_core::ThreadId::new(thread.agent_path).map_err(Into::into)
    }
}

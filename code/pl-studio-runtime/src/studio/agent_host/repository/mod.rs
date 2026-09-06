use std::collections::BTreeSet;

use pl_core::{
    AgentSubmissionPage, RestoredAgentRuntime, ThreadCommit, ThreadId, ThreadRepository,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::thread;

mod billing;
mod commit_apply;
mod context;
mod conversion;
mod input_metadata;
mod inputs;
pub(super) mod labels;
mod projection;
mod restore;
mod session_timeline;
mod submissions;
mod write_behind;

use context::SessionSnapshotAuditError;
use submissions::list_thread_submissions;

use commit_apply::{ApplyCommitOutcome, apply_state_commit};
pub(in crate::studio) use write_behind::ThreadWriteBehindWriter;

/// Studio 单库对 canonical Thread 状态的 write-behind repository。
///
/// commit 只进入 [`ThreadWriteBehindWriter`] 队列，由后台批量事务落库；
/// 内存 actor state 是唯一权威实例。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentRepository {
    store: StudioStore,
    writer: ThreadWriteBehindWriter,
    model_performance: Option<crate::studio::runtime::ModelPerformanceOwner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::studio) struct StudioSessionRecoveryFailure {
    pub project_id: String,
    pub root_thread_id: String,
    pub agent_thread_id: String,
    pub detail: String,
}

impl StudioAgentRepository {
    pub(in crate::studio) fn with_writer_and_performance(
        store: StudioStore,
        writer: ThreadWriteBehindWriter,
        model_performance: crate::studio::runtime::ModelPerformanceOwner,
    ) -> Self {
        Self {
            writer,
            store,
            model_performance: Some(model_performance),
        }
    }

    /// write-behind writer 句柄；关机排空与进度查询使用。
    pub(in crate::studio) fn writer(&self) -> &ThreadWriteBehindWriter {
        &self.writer
    }
}

impl ThreadRepository for StudioAgentRepository {
    type Error = PureError;

    /// 只恢复启动钉住集合：存在 pending input、pending Interaction、活动 Turn
    /// 或被活动协作/续轮引用的 Thread。其余 Thread 惰性驻留。
    async fn restore_runtime(&self) -> Result<Vec<RestoredAgentRuntime>, Self::Error> {
        let pinned = self.pinned_thread_ids().await?;
        if pinned.is_empty() {
            return Ok(Vec::new());
        }
        let models = thread::Entity::find()
            .filter(thread::Column::RuntimeRevision.is_not_null())
            .filter(thread::Column::Id.is_in(pinned))
            .order_by_asc(thread::Column::CreatedAt)
            .order_by_asc(thread::Column::Id)
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        let parents = self.ancestor_parents(&models).await?;
        let blocked_roots = self
            .session_recovery_failures(&models)
            .await?
            .into_iter()
            .map(|failure| failure.root_thread_id)
            .collect::<BTreeSet<_>>();
        let mut restored = Vec::with_capacity(models.len());
        for model in models {
            if blocked_roots.contains(&model.root_thread_id) {
                tracing::warn!(
                    root_thread_id = %model.root_thread_id,
                    agent_thread_id = %model.id,
                    "skipping agent tree with an invalid durable session snapshot"
                );
                continue;
            }
            restored.push(self.restore_model(model, &parents).await?);
        }
        Ok(restored)
    }

    /// 按需恢复单个已注册 Thread；不存在、未注册 runtime 或 session 损坏时返回 `None`。
    async fn restore_thread(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Option<RestoredAgentRuntime>, Self::Error> {
        let Some(model) = thread::Entity::find_by_id(thread_id.to_string())
            .one(self.store.database())
            .await
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if model.runtime_revision.is_none() {
            return Ok(None);
        }
        match self.audit_thread_recovery_payloads(&model.id).await {
            Ok(()) => {}
            Err(SessionSnapshotAuditError::Corrupt(_)) => {
                tracing::warn!(
                    agent_thread_id = %model.id,
                    "refusing to lazily restore a thread with an invalid durable payload"
                );
                return Ok(None);
            }
            Err(SessionSnapshotAuditError::Fatal(error)) => return Err(error),
        }
        let parents = self.ancestor_parents(std::slice::from_ref(&model)).await?;
        Ok(Some(self.restore_model(model, &parents).await?))
    }

    fn record_committed(&self, commit: ThreadCommit) {
        self.writer.record_thread(commit.clone());
        if let (Some(owner), Some(inference)) =
            (&self.model_performance, commit.facts.inference.as_ref())
        {
            let Some(projection) = commit.facts.projection_snapshot.as_ref() else {
                tracing::error!("inference commit is missing its canonical Thread projection");
                return;
            };
            if let Err(error) = owner.record_inference(
                &projection.thread.root_thread_id,
                commit.agent_id.as_str(),
                &inference.billing,
            ) {
                tracing::error!(
                    agent_id = %commit.agent_id,
                    inference_id = %inference.billing.inference_id,
                    error = %error,
                    "model performance projection rejected an admitted Thread fact"
                );
            }
        }
    }

    fn is_durable(&self, thread_id: &ThreadId, revision: u64) -> bool {
        self.writer.is_durable(thread_id.as_str(), revision)
    }

    async fn await_durable(&self, thread_id: &ThreadId, revision: u64) -> Result<(), Self::Error> {
        self.writer
            .await_durable(thread_id.as_str(), revision)
            .await
    }

    fn pending_commit_count(&self) -> usize {
        self.writer.pending_commit_count()
    }

    async fn list_submissions(
        &self,
        thread_id: &ThreadId,
        offset: usize,
        limit: usize,
    ) -> Result<AgentSubmissionPage, Self::Error> {
        list_thread_submissions(&self.store, thread_id, offset, limit).await
    }

    async fn list_agent_session(
        &self,
        query: pl_core::AgentSessionTimelineQuery,
    ) -> Result<pl_core::AgentSessionTimelineRepositoryPage, Self::Error> {
        session_timeline::list_agent_session(&self.store, query).await
    }
}

pub(super) fn u64_from_i64(value: i64) -> Result<u64, PureError> {
    u64::try_from(value).map_err(|error| store_error(error.to_string()))
}

pub(super) fn i64_from_u64(value: u64) -> Result<i64, PureError> {
    i64::try_from(value).map_err(|error| store_error(error.to_string()))
}

pub(super) fn store_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

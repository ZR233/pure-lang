//! Studio product adapters around the core-owned session repository.

use crate::PureError;
use crate::studio::StudioStore;
use pl_core::{
    AgentSubmissionPage, RestoredAgentRuntime, ThreadCommit, ThreadId, ThreadRepository,
};

mod conversion;
pub(super) mod labels;
mod write_behind;

pub(in crate::studio) use write_behind::ThreadWriteBehindWriter;

#[derive(Clone)]
pub(in crate::studio) struct StudioAgentRepository {
    store: StudioStore,
    writer: ThreadWriteBehindWriter,
    model_performance: crate::studio::runtime::ModelPerformanceOwner,
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
            store,
            writer,
            model_performance,
        }
    }

    pub(in crate::studio) fn writer(&self) -> &ThreadWriteBehindWriter {
        &self.writer
    }

    pub(in crate::studio) async fn audit_registered_sessions(
        &self,
    ) -> Result<Vec<StudioSessionRecoveryFailure>, PureError> {
        let registered: std::collections::BTreeSet<_> = self
            .store
            .registered_session_ids()
            .await
            .map_err(store_error)?
            .into_iter()
            .collect();
        let mut ids: std::collections::BTreeSet<_> = self
            .store
            .sessions()
            .session_ids()
            .await
            .map_err(store_error)?
            .into_iter()
            .collect();
        ids.extend(registered.iter().cloned());
        let mut failures = Vec::new();
        for id in ids {
            let thread = self
                .store
                .read_thread_association(&id)
                .await
                .map_err(store_error)?;
            let (detail, fallback_project) = match self.store.sessions().read_session(&id).await {
                Err(error) => (Some(error.to_string()), None),
                Ok(None) if registered.contains(&id) => (Some("Registered session data is missing; refusing to recreate an empty conversation".into()), None),
                Ok(Some(agent)) => {
                    let project = agent.state.session.metadata.project_id.clone();
                    let detail = match &thread {
                        None => Some("Durable session has no product association; data and physical resources were preserved".into()),
                        Some(thread) if thread.agent_path != agent.state.snapshot.identity.id.as_str()
                            || thread.parent_thread_id.as_deref() != agent.state.snapshot.identity.parent_id.as_ref().map(|id| id.as_str()) =>
                            Some("Session and product ownership disagree; refusing activation".into()),
                        Some(_) => None,
                    };
                    (detail, project)
                }
                Ok(None) => (None,None),
            };
            if let Some(detail) = detail {
                failures.push(StudioSessionRecoveryFailure {
                    project_id: thread
                        .as_ref()
                        .map(|thread| thread.project_id.clone())
                        .or(fallback_project)
                        .unwrap_or_default(),
                    root_thread_id: thread
                        .as_ref()
                        .map(|thread| thread.root_thread_id.clone())
                        .unwrap_or_else(|| id.clone()),
                    agent_thread_id: id,
                    detail,
                });
            }
        }
        Ok(failures)
    }
}

impl ThreadRepository for StudioAgentRepository {
    type Error = PureError;

    async fn restore_runtime(&self) -> Result<Vec<RestoredAgentRuntime>, Self::Error> {
        self.store
            .sessions()
            .restore_runtime()
            .await
            .map_err(store_error)
    }

    async fn restore_thread(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Option<RestoredAgentRuntime>, Self::Error> {
        self.store
            .sessions()
            .restore_thread(thread_id)
            .await
            .map_err(store_error)
    }

    async fn read_tool_task_result(
        &self,
        thread_id: &ThreadId,
        task_id: &str,
    ) -> Result<Option<pl_core::session_runtime::ToolTaskResult>, Self::Error> {
        self.store
            .sessions()
            .read_tool_task_result(thread_id, task_id)
            .await
            .map_err(store_error)
    }

    fn record_committed(&self, commit: ThreadCommit) {
        self.store.sessions().record_committed(commit.clone());
        if !commit.facts.runtime_events.is_empty() {
            self.writer
                .record_directory(crate::studio::store::directory::DirectoryDelta {
                    session_activity: vec![(
                        commit.agent_id.to_string(),
                        commit.next_state.snapshot.updated_at,
                    )],
                    ..Default::default()
                });
        }
        if commit.expected_revision.is_none() {
            self.writer
                .record_directory(crate::studio::store::directory::DirectoryDelta {
                    session_registrations: vec![commit.agent_id.to_string()],
                    ..Default::default()
                });
        }
        if let Some(inference) = commit.facts.inference.as_ref()
            && let Some(projection) = commit.facts.projection_snapshot.as_ref()
            && let Err(error) = self.model_performance.record_inference(
                &projection.thread.root_thread_id,
                commit.agent_id.as_str(),
                &inference.billing,
            )
        {
            tracing::error!(agent_id = %commit.agent_id, %error, "model performance rejected admitted session fact");
        }
    }

    fn is_durable(&self, thread_id: &ThreadId, revision: u64) -> bool {
        self.store
            .sessions()
            .is_durable(thread_id.as_str(), revision)
            && !self.writer.has_pending_directory(thread_id.as_str())
    }
    async fn await_durable(&self, thread_id: &ThreadId, revision: u64) -> Result<(), Self::Error> {
        self.store
            .sessions()
            .await_durable(thread_id.as_str(), revision)
            .await
            .map_err(store_error)
    }
    fn pending_commit_count(&self) -> usize {
        self.store.sessions().persistence().pending_commits
    }
    async fn flush(&self) -> Result<(), Self::Error> {
        self.store.sessions().flush().await.map_err(store_error)
    }
    async fn shutdown(&self) -> Result<(), Self::Error> {
        self.store.sessions().shutdown().await.map_err(store_error)
    }

    async fn list_submissions(
        &self,
        thread_id: &ThreadId,
        offset: usize,
        limit: usize,
    ) -> Result<AgentSubmissionPage, Self::Error> {
        self.store
            .sessions()
            .list_submissions(thread_id, offset, limit)
            .await
            .map_err(store_error)
    }
    async fn list_agent_session(
        &self,
        query: pl_core::AgentSessionTimelineQuery,
    ) -> Result<pl_core::AgentSessionTimelineRepositoryPage, Self::Error> {
        self.store
            .sessions()
            .list_agent_session(query)
            .await
            .map_err(store_error)
    }
}

pub(super) fn store_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

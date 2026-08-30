use pl_core::{
    AgentCommittedEvent, AgentRuntimeEvent, AgentRuntimeEventKind, AgentSnapshot, AgentState,
};

use crate::StudioAgentDirectoryEntry;
use crate::studio::ProductEventBus;

use super::super::resources::StudioAgentResources;

pub(super) struct StudioAgentEventProjector {
    pub(super) resources: StudioAgentResources,
    pub(super) product_events: ProductEventBus,
}

pub(super) struct StudioAgentProjectionFailure {
    pub(super) stage: &'static str,
    pub(super) source: anyhow::Error,
}

trait ProjectionResultExt<T> {
    fn at(self, stage: &'static str) -> Result<T, StudioAgentProjectionFailure>;
}

impl<T, E> ProjectionResultExt<T> for Result<T, E>
where
    E: Into<anyhow::Error>,
{
    fn at(self, stage: &'static str) -> Result<T, StudioAgentProjectionFailure> {
        self.map_err(|source| StudioAgentProjectionFailure {
            stage,
            source: source.into(),
        })
    }
}

impl StudioAgentEventProjector {
    pub(super) async fn project(
        &self,
        committed: AgentCommittedEvent,
    ) -> Result<(), StudioAgentProjectionFailure> {
        let thread_id = self.resources.thread_id(&committed.agent_id).await;
        for event in committed.runtime_events {
            self.project_runtime_event(event, thread_id.as_deref())
                .await?;
        }
        Ok(())
    }

    async fn project_runtime_event(
        &self,
        event: AgentRuntimeEvent,
        thread_id: Option<&str>,
    ) -> Result<(), StudioAgentProjectionFailure> {
        let snapshot = match event.kind {
            AgentRuntimeEventKind::Registered { snapshot }
            | AgentRuntimeEventKind::StateChanged { snapshot }
            | AgentRuntimeEventKind::ThreadOpened { snapshot, .. }
            | AgentRuntimeEventKind::TurnActivityChanged { snapshot, .. }
            | AgentRuntimeEventKind::Faulted { snapshot, .. }
            | AgentRuntimeEventKind::TurnQueued { snapshot, .. }
            | AgentRuntimeEventKind::TurnStarted { snapshot, .. }
            | AgentRuntimeEventKind::TurnFinished { snapshot, .. }
            | AgentRuntimeEventKind::RecoveryCancelledTurn { snapshot, .. } => *snapshot,
        };
        self.emit_agent_snapshot(thread_id, snapshot).await
    }

    async fn emit_agent_snapshot(
        &self,
        thread_id: Option<&str>,
        snapshot: AgentSnapshot,
    ) -> Result<(), StudioAgentProjectionFailure> {
        let Some(thread_id) = thread_id else {
            return Ok(());
        };
        let resource = self.resources.get(&snapshot.identity.id).await;
        if let Some(mut thread) = self.product_events.thread_snapshot(thread_id) {
            thread.status = super::super::repository::labels::thread_status(&snapshot.state);
            thread.updated_at = thread.updated_at.max(snapshot.updated_at);
            let progress = snapshot.progress.clone();
            let summary_age_seconds = u64::try_from(
                crate::studio::ids::unix_seconds()
                    .saturating_sub(
                        snapshot
                            .progress
                            .as_ref()
                            .map_or(snapshot.updated_at, |progress| progress.updated_at),
                    )
                    .max(0),
            )
            .unwrap_or_default();
            self.product_events
                .update_agent_directory(StudioAgentDirectoryEntry {
                    id: snapshot.identity.id.to_string(),
                    thread_id: thread.id.clone(),
                    root_thread_id: thread.root_thread_id.clone(),
                    path: snapshot.identity.id.to_string(),
                    parent_path: snapshot
                        .identity
                        .parent_id
                        .as_ref()
                        .map(ToString::to_string),
                    role: snapshot.identity.role.to_string(),
                    task: resource.as_ref().map_or_else(
                        || thread.title.clone(),
                        |resource| resource.assignment_name.clone(),
                    ),
                    summary: snapshot
                        .progress
                        .as_ref()
                        .map(|progress| progress.report.summary.clone()),
                    depth: snapshot.identity.depth,
                    state: snapshot.state.clone(),
                    progress,
                    updated_at: snapshot.updated_at,
                    summary_age_seconds,
                })
                .await;
            self.product_events
                .apply_thread_delta(vec![thread], Vec::new())
                .await
                .at("emitThreadDirectory")?;
        }
        if matches!(snapshot.state, AgentState::Closed(_)) {
            self.resources
                .release_after_close(&snapshot.identity.id)
                .await;
        }
        Ok(())
    }
}

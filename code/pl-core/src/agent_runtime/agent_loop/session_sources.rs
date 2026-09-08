use crate::session_runtime::{SessionSourceError, SessionSourceFailure, SessionWakeEvent};

use super::super::{AgentRuntimeError, AgentRuntimeHost, AgentRuntimeResult};
use super::AgentLoop;

impl<H: AgentRuntimeHost> AgentLoop<H> {
    pub(super) async fn initialize_session_sources(&mut self) -> AgentRuntimeResult<()> {
        let desired: std::collections::BTreeSet<_> = self
            .session_runtime
            .source_ids()
            .map(|id| format!("source:{id}"))
            .collect();
        let mut next = self.state.clone();
        let previous: Vec<_> = next
            .session
            .inbox
            .reservations()
            .filter(|id| id.starts_with("source:"))
            .map(str::to_owned)
            .collect();
        for id in previous {
            if !desired.contains(&id) {
                next.session.inbox.release_reservation(&id);
            }
        }
        for id in desired {
            next.session
                .inbox
                .reserve(&id)
                .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        }
        if next.session.inbox.snapshot() != self.state.session.inbox.snapshot() {
            self.commit_session_task_state(next).await?;
        }
        self.session_runtime.start_sources();
        Ok(())
    }

    pub(super) async fn finish_session_source(
        &mut self,
        id: &str,
        result: Result<(), SessionSourceError>,
    ) -> AgentRuntimeResult<()> {
        let mut next = self.state.clone();
        let reservation = format!("source:{id}");
        if self.session_runtime.is_closing() && result.is_ok() {
            next.session.inbox.release_reservation(&reservation);
        } else {
            let message = match result {
                Ok(()) => "Event source stopped before session closure".to_owned(),
                Err(error) => error.to_string(),
            };
            next.session
                .inbox
                .publish_reserved(
                    &reservation,
                    id,
                    SessionWakeEvent::SourceFailed(SessionSourceFailure {
                        code: "sourceStopped".into(),
                        message: message.chars().take(4096).collect(),
                    }),
                    super::super::state::unix_timestamp(),
                )
                .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        }
        self.commit_session_task_state(next).await?;
        self.wake_session_waiter();
        Ok(())
    }

    pub(super) async fn drain_session_sources(&mut self) -> AgentRuntimeResult<()> {
        self.session_runtime.cancel();
        let deadline = tokio::time::Instant::now() + self.cancel_grace;
        while self.session_runtime.has_running_sources() {
            let completion = tokio::time::timeout_at(deadline, self.session_runtime.next_source())
                .await
                .map_err(|_| {
                    AgentRuntimeError::Lifecycle("source cleanup remains pending".into())
                })?;
            if let Some((id, result)) = completion {
                self.finish_session_source(&id, result).await?;
            }
        }
        Ok(())
    }
}

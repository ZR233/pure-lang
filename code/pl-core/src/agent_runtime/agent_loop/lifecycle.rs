use super::super::host::{DurableCommitFacts, PersistenceClass, ThreadProjectionCommit};
use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentCommand, AgentRuntimeEventKind, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshot,
    AgentSnapshotTransition, AgentState, ThreadMutation,
};
use super::AgentLoop;
use super::commit::{CommitPublication, PendingCommit};
use crate::AgentRoleId;
use crate::thread_event::project_thread_facts;

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn recover_faulted(&mut self) -> AgentRuntimeResult<AgentSnapshot> {
        if self.session_runtime.is_closing() {
            return Err(AgentRuntimeError::Lifecycle("session resources are shutting down; close or reactivate the session instead of resuming this owner".into()));
        }
        let AgentState::Faulted(faulted) = &self.state.snapshot.state else {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.state.clone(),
            ));
        };
        if !faulted.classification().is_recoverable() {
            return Err(AgentRuntimeError::InvalidInput(
                "faulted Agent requires manual recovery because its aggregate is not verified"
                    .to_string(),
            ));
        }
        self.task_resources.resume_settlement();
        self.release_turn_deliveries(None).await?;
        let mut next = self.state.clone();
        next.pending_inputs
            .retain(|input| input.delivery_state.is_pending());
        next.active_input = None;
        next.refresh_mailbox_snapshot();
        next.snapshot.progress = None;
        next.snapshot
            .transition(AgentCommand::RecoverFaulted {
                target: super::super::AgentRecoveryTarget::Idle,
            })
            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        self.commit_transition(
            super::persist::TransitionCommit::new(next).settlement(),
            |snapshot| AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(snapshot),
            },
        )
        .await?;
        self.start_session_tasks().await?;
        Ok(self.state.snapshot.clone())
    }

    pub(super) async fn reconfigure_idle_role(
        &mut self,
        role: AgentRoleId,
    ) -> AgentRuntimeResult<AgentSnapshot> {
        if self.state.snapshot.identity.role == role {
            return Ok(self.state.snapshot.clone());
        }
        if !self.state.snapshot.state.is_operational() {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.state.clone(),
            ));
        }
        if self.active.is_some()
            || !self.state.snapshot.state.is_idle()
            || self.state.active_input.is_some()
            || !self.state.pending_inputs.is_empty()
        {
            return Err(AgentRuntimeError::InvalidInput(
                "agent role can only change while the Thread is idle with no pending input"
                    .to_string(),
            ));
        }

        let mut next = self.state.clone();
        next.snapshot.identity.role = role;
        self.commit_transition(
            super::persist::TransitionCommit::new(next).settlement(),
            |snapshot| AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(snapshot),
            },
        )
        .await?;
        Ok(self.state.snapshot.clone())
    }

    pub(super) async fn change_idle_thread_mode(
        &mut self,
        mode_id: pl_protocol::ThreadModeId,
    ) -> AgentRuntimeResult<AgentSnapshot> {
        if self.state.snapshot.identity.parent_id.is_some() {
            return Err(AgentRuntimeError::InvalidInput(
                "only a root Thread can change Thread Mode".to_string(),
            ));
        }
        if !self.state.snapshot.state.is_operational() {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.state.clone(),
            ));
        }
        if self.active.is_some()
            || !self.state.snapshot.state.is_idle()
            || self.state.active_input.is_some()
            || !self.state.pending_inputs.is_empty()
        {
            return Err(AgentRuntimeError::InvalidInput(
                "Thread Mode can only change while the root Thread is idle with no pending input"
                    .to_string(),
            ));
        }

        let now = unix_timestamp();
        let workflow = crate::archive_workflow_for_mode_change(
            self.state.session.session.workflow().cloned(),
            &mode_id,
            now,
        )
        .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        let mut next = self.state.clone();
        if !next.session.session.replace_workflow(workflow.clone()) {
            return Ok(self.state.snapshot.clone());
        }

        let thread_id = next.snapshot.identity.id.clone();
        let expected_revision = self.state.snapshot.revision;
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = now;
        let current = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let mut runtime = current
            .runtime
            .clone()
            .unwrap_or_else(|| crate::thread_event::empty_runtime(thread_id.as_str()));
        runtime.workflow = workflow
            .as_ref()
            .map(pl_protocol::WorkflowRuntimeSnapshot::from);
        runtime.updated_at = now;
        let projected = project_thread_facts(
            thread_id.as_str(),
            &current,
            vec![crate::ThreadNotificationFact::durable(
                now,
                pl_protocol::ThreadNotification::ThreadRuntimeUpdated {
                    runtime: Box::new(runtime),
                },
            )],
        );
        let projected_thread = self
            .runtime
            .thread_events
            .project(thread_id.as_str(), &projected.notifications)
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        next.session.thread_revision = projected.through_revision;
        let notifications = projected_thread.notifications.clone();
        let projection = ThreadProjectionCommit {
            snapshot: projected_thread.snapshot,
            notifications: notifications.clone(),
        };
        let facts =
            DurableCommitFacts::from_state(&next, Vec::new(), Vec::new(), Some(projection), None);
        self.commit_and_publish(
            PendingCommit::new(
                next,
                facts,
                ThreadMutation::ReplaceThread {
                    thread_id: thread_id.clone(),
                },
            )
            .persistence(PersistenceClass::Settlement)
            .publish(
                CommitPublication::new(Some(thread_id), None)
                    .with_thread_notifications(notifications),
            ),
        )
        .await?;
        Ok(self.state.snapshot.clone())
    }

    pub(super) async fn shutdown(&mut self) -> AgentRuntimeResult<AgentSnapshot> {
        self.dispatch_enabled = false;
        if self.closing.is_some() || matches!(self.state.snapshot.state, AgentState::Closing(_)) {
            return self.finish_close_for_shutdown().await;
        }
        self.drain_session_sources().await?;
        self.drain_session_tasks().await?;
        if self.active.is_none() {
            self.session_runtime.release_tools();
            return Ok(self.state.snapshot.clone());
        }
        let result = self
            .interrupt_active_turn(pl_protocol::TurnCancellationCause::RuntimeShutdown)
            .await;
        if let Err(error) = &result {
            self.fault(error.to_string()).await;
        }
        result.map(|()| {
            self.session_runtime.release_tools();
            self.state.snapshot.clone()
        })
    }
}

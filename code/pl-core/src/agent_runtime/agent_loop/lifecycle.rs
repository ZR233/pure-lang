use super::super::host::{
    AgentLifecycleAdapter, DurableCommitFacts, PersistenceClass, ThreadProjectionCommit,
};
use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentCommand, AgentRuntimeEventKind, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshot,
    AgentSnapshotTransition, AgentState, CloseLifecycleRequest, ThreadMutation,
};
use super::AgentLoop;
use super::commit::{CommitPublication, PendingCommit};
use crate::AgentRoleId;
use crate::thread_event::project_thread_facts;

enum CloseCompensation {
    Restored,
    Faulted { reason: String },
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn recover_faulted(&mut self) -> AgentRuntimeResult<AgentSnapshot> {
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

    pub(super) async fn close(
        &mut self,
        workspace_disposition: pl_protocol::AgentWorkspaceDisposition,
    ) -> AgentRuntimeResult<AgentSnapshot> {
        if matches!(self.state.snapshot.state, AgentState::Closed(_)) {
            if workspace_disposition == pl_protocol::AgentWorkspaceDisposition::Cleanup {
                let lease = self
                    .host
                    .lifecycle()
                    .prepare_close(CloseLifecycleRequest {
                        agent: self.state.snapshot.clone(),
                        workspace_disposition,
                    })
                    .await
                    .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
                if let Err(error) = self.host.lifecycle().commit_close(&lease).await {
                    let rollback = self.host.lifecycle().rollback_close(lease).await;
                    let reason = match rollback {
                        Ok(()) => error.to_string(),
                        Err(rollback_error) => {
                            format!("{error}; close rollback failed: {rollback_error}")
                        }
                    };
                    return Err(AgentRuntimeError::Lifecycle(reason));
                }
            }
            return Ok(self.state.snapshot.clone());
        }
        let lease = self
            .host
            .lifecycle()
            .prepare_close(CloseLifecycleRequest {
                agent: self.state.snapshot.clone(),
                workspace_disposition,
            })
            .await
            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        if self.active.is_some()
            && let Err(error) = self
                .interrupt_active_turn(pl_protocol::TurnCancellationCause::AgentClosed)
                .await
        {
            let rollback = self.host.lifecycle().rollback_close(lease).await;
            let reason = match rollback {
                Ok(()) => error.to_string(),
                Err(rollback_error) => {
                    format!("{error}; close rollback failed: {rollback_error}")
                }
            };
            self.fault(reason.clone()).await;
            return Err(AgentRuntimeError::Repository(reason));
        }
        if let Err(error) = self.flush_pending_traces().await {
            let rollback = self.host.lifecycle().rollback_close(lease).await;
            let reason = match rollback {
                Ok(()) => error.to_string(),
                Err(rollback_error) => {
                    format!("{error}; close rollback failed: {rollback_error}")
                }
            };
            self.fault(reason.clone()).await;
            return Err(AgentRuntimeError::Repository(reason));
        }
        let mut closing = self.state.clone();
        closing
            .snapshot
            .transition(AgentCommand::BeginClose)
            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        closing.active_input = None;
        if let Err(error) = self
            .commit_transition(
                super::persist::TransitionCommit::new(closing).settlement(),
                |snapshot| AgentRuntimeEventKind::StateChanged {
                    snapshot: Box::new(snapshot),
                },
            )
            .await
        {
            if let Err(rollback_error) = self.host.lifecycle().rollback_close(lease).await {
                let reason = format!(
                    "failed to persist closing state: {error}; close rollback failed: {rollback_error}"
                );
                self.fault(reason.clone()).await;
                return Err(AgentRuntimeError::Lifecycle(reason));
            }
            return Err(error);
        }
        self.stop_active_turn();
        let close_result = self
            .host
            .lifecycle()
            .commit_close(&lease)
            .await
            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()));
        if let Err(error) = close_result {
            let rollback = self.host.lifecycle().rollback_close(lease).await;
            let (return_error, compensation) = match rollback {
                Ok(()) => (error, CloseCompensation::Restored),
                Err(rollback_error) => {
                    let reason = format!("{error}; close rollback failed: {rollback_error}");
                    (
                        AgentRuntimeError::Lifecycle(reason.clone()),
                        CloseCompensation::Faulted { reason },
                    )
                }
            };
            self.persist_close_compensation(compensation).await?;
            return Err(return_error);
        }
        let mut closed = self.state.clone();
        closed.pending_inputs.clear();
        closed.active_input = None;
        closed
            .snapshot
            .transition(AgentCommand::Close)
            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        closed.snapshot.pending_inputs = 0;
        if let Err(error) = self
            .commit_transition(
                super::persist::TransitionCommit::new(closed).settlement(),
                |snapshot| AgentRuntimeEventKind::StateChanged {
                    snapshot: Box::new(snapshot),
                },
            )
            .await
        {
            let rollback = self.host.lifecycle().rollback_close(lease).await;
            let compensation = match rollback {
                Ok(()) => CloseCompensation::Restored,
                Err(rollback_error) => CloseCompensation::Faulted {
                    reason: format!(
                        "failed to persist closed state: {error}; close rollback failed: {rollback_error}"
                    ),
                },
            };
            self.persist_close_compensation(compensation).await?;
            return Err(error);
        }
        Ok(self.state.snapshot.clone())
    }

    async fn persist_close_compensation(
        &mut self,
        compensation: CloseCompensation,
    ) -> AgentRuntimeResult<()> {
        let restored = matches!(compensation, CloseCompensation::Restored);
        let mut next = self.state.clone();
        let next_turn_id = next.triggering_turn_id();
        let command = match &compensation {
            CloseCompensation::Restored => AgentCommand::Restore { next_turn_id },
            CloseCompensation::Faulted { reason } => AgentCommand::Fault {
                error: pl_protocol::StateError {
                    code: "agentCloseCompensationFailed".to_string(),
                    message: reason.clone(),
                    retryable: false,
                },
                turn_id: None,
                classification: super::super::AgentFaultClassification::AggregateCorruption,
            },
        };
        next.snapshot
            .transition(command)
            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        let event_compensation = compensation;
        if let Err(error) = self
            .commit_transition(
                super::persist::TransitionCommit::new(next).settlement(),
                move |snapshot| match event_compensation {
                    CloseCompensation::Restored => AgentRuntimeEventKind::StateChanged {
                        snapshot: Box::new(snapshot),
                    },
                    CloseCompensation::Faulted { reason } => AgentRuntimeEventKind::Faulted {
                        reason,
                        snapshot: Box::new(snapshot),
                    },
                },
            )
            .await
        {
            self.fault(error.to_string()).await;
            return Err(error);
        }
        if restored && self.dispatch_enabled && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
        Ok(())
    }

    pub(super) async fn shutdown(&mut self) -> AgentRuntimeResult<AgentSnapshot> {
        self.dispatch_enabled = false;
        if self.active.is_none() {
            return Ok(self.state.snapshot.clone());
        }
        let result = self
            .interrupt_active_turn(pl_protocol::TurnCancellationCause::RuntimeShutdown)
            .await;
        if let Err(error) = &result {
            self.fault(error.to_string()).await;
        }
        result.map(|()| self.state.snapshot.clone())
    }
}

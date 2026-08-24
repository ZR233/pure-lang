use super::super::host::AgentLifecycleAdapter;
use super::super::state::AgentRuntimeError;
use super::super::{
    AgentCommand, AgentRuntimeEventKind, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshot,
    AgentState, CloseLifecycleRequest,
};
use super::AgentLoop;
use crate::AgentRoleId;

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

    pub(super) async fn close(&mut self) -> AgentRuntimeResult<AgentSnapshot> {
        if matches!(self.state.snapshot.state, AgentState::Closed(_)) {
            return Ok(self.state.snapshot.clone());
        }
        let lease = self
            .host
            .lifecycle()
            .prepare_close(CloseLifecycleRequest {
                agent: self.state.snapshot.clone(),
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
        if let Err(error) = self.host.lifecycle().commit_close(&lease).await {
            let rollback = self.host.lifecycle().rollback_close(lease).await;
            let (reason, compensation) = match rollback {
                Ok(()) => (error.to_string(), CloseCompensation::Restored),
                Err(rollback_error) => {
                    let reason = format!("{error}; close rollback failed: {rollback_error}");
                    (reason.clone(), CloseCompensation::Faulted { reason })
                }
            };
            self.persist_close_compensation(compensation).await?;
            return Err(AgentRuntimeError::Lifecycle(reason));
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

    pub(super) async fn shutdown(&mut self) -> AgentRuntimeResult<()> {
        if self.active.is_none() {
            return Ok(());
        }
        let result = self
            .interrupt_active_turn(pl_protocol::TurnCancellationCause::RuntimeShutdown)
            .await;
        if let Err(error) = &result {
            self.fault(error.to_string()).await;
        }
        result
    }
}

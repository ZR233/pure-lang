use super::super::host::AgentLifecycleAdapter;
use super::super::state::AgentRuntimeError;
use super::super::{
    AgentActivityState, AgentLifecycleState, AgentRuntimeEventKind, AgentRuntimeHost,
    AgentRuntimeResult, AgentSnapshot, CloseLifecycleRequest,
};
use super::AgentLoop;

enum CloseCompensation {
    Restored,
    Faulted { reason: String },
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn close(&mut self) -> AgentRuntimeResult<AgentSnapshot> {
        if self.state.snapshot.lifecycle == AgentLifecycleState::Closed {
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
            && let Err(error) = self.interrupt_active_turn("agent_close_requested").await
        {
            let rollback = self.host.lifecycle().rollback_close(lease).await;
            let reason = match rollback {
                Ok(()) => error.to_string(),
                Err(rollback_error) => {
                    format!("{error}; close rollback failed: {rollback_error}")
                }
            };
            self.fault_in_memory(reason.clone());
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
            self.fault_in_memory(reason.clone());
            return Err(AgentRuntimeError::Repository(reason));
        }
        let mut closing = self.state.clone();
        closing.snapshot.lifecycle = AgentLifecycleState::Closing;
        closing.snapshot.activity = AgentActivityState::Idle;
        closing.snapshot.active_turn_id = None;
        closing.active_input = None;
        if let Err(error) = self
            .commit_transition(closing, Vec::new(), |snapshot| {
                AgentRuntimeEventKind::StateChanged { snapshot }
            })
            .await
        {
            if let Err(rollback_error) = self.host.lifecycle().rollback_close(lease).await {
                let reason = format!(
                    "failed to persist closing state: {error}; close rollback failed: {rollback_error}"
                );
                self.fault_in_memory(reason.clone());
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
        closed.snapshot.lifecycle = AgentLifecycleState::Closed;
        closed.snapshot.activity = AgentActivityState::Idle;
        closed.snapshot.active_turn_id = None;
        closed.snapshot.pending_inputs = 0;
        if let Err(error) = self
            .commit_transition(closed, Vec::new(), |snapshot| {
                AgentRuntimeEventKind::StateChanged { snapshot }
            })
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
        next.snapshot.active_turn_id = None;
        next.snapshot.lifecycle = match &compensation {
            CloseCompensation::Restored => AgentLifecycleState::Active,
            CloseCompensation::Faulted { .. } => AgentLifecycleState::Faulted,
        };
        next.snapshot.activity = match &compensation {
            CloseCompensation::Restored if self.dispatch_enabled && next.has_triggering_input() => {
                AgentActivityState::Queued
            }
            CloseCompensation::Restored | CloseCompensation::Faulted { .. } => {
                AgentActivityState::Idle
            }
        };
        let event_compensation = compensation;
        if let Err(error) = self
            .commit_transition(next, Vec::new(), move |snapshot| match event_compensation {
                CloseCompensation::Restored => AgentRuntimeEventKind::StateChanged { snapshot },
                CloseCompensation::Faulted { reason } => {
                    AgentRuntimeEventKind::Faulted { reason, snapshot }
                }
            })
            .await
        {
            self.fault_in_memory(error.to_string());
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
        self.interrupt_active_turn("runtime_shutdown")
            .await
            .inspect_err(|error| self.fault_in_memory(error.to_string()))
    }
}

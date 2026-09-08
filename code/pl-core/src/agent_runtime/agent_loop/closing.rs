use futures::{FutureExt, future::BoxFuture};

use super::super::{
    AgentCommand, AgentDirectorySubscription, AgentLifecycleAdapter, AgentRuntimeError,
    AgentRuntimeEventKind, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshot,
    AgentSnapshotTransition, AgentState, CloseLifecycleRequest,
};
use super::AgentLoop;

type CloseLease<H> = <<H as AgentRuntimeHost>::Lifecycle as AgentLifecycleAdapter>::CloseLease;

pub(super) struct SessionClose<L> {
    disposition: pl_protocol::AgentWorkspaceDisposition,
    phase: ClosePhase<L>,
    updates: AgentDirectorySubscription,
    cancel_deadline: Option<tokio::time::Instant>,
}

enum ClosePhase<L> {
    Preparing(BoxFuture<'static, AgentRuntimeResult<L>>),
    Draining(L),
    Cleaning(BoxFuture<'static, (L, AgentRuntimeResult<()>)>),
    Failed(Option<L>),
}

pub(super) enum CloseEvent<L> {
    Prepared(AgentRuntimeResult<L>),
    Cleaned(L, AgentRuntimeResult<()>),
    Wake,
    DirectoryClosed,
    CancelDeadline,
}

impl<L: Send + 'static> SessionClose<L> {
    pub(super) async fn next_event(&mut self) -> CloseEvent<L> {
        let deadline = self.cancel_deadline;
        let event = async {
            match &mut self.phase {
                ClosePhase::Preparing(future) => CloseEvent::Prepared(future.await),
                ClosePhase::Cleaning(future) => {
                    let (lease, result) = future.await;
                    CloseEvent::Cleaned(lease, result)
                }
                ClosePhase::Draining(_) => match self.updates.changed().await {
                    Ok(_) => CloseEvent::Wake,
                    Err(_) => CloseEvent::DirectoryClosed,
                },
                ClosePhase::Failed(_) => std::future::pending().await,
            }
        };
        tokio::select! {
            event = event => event,
            _ = async { match deadline { Some(at) => tokio::time::sleep_until(at).await, None => std::future::pending().await } } => {
                self.cancel_deadline = None;
                CloseEvent::CancelDeadline
            }
        }
    }
}

pub(super) async fn next_close_event<L: Send + 'static>(
    close: &mut Option<SessionClose<L>>,
) -> CloseEvent<L> {
    match close {
        Some(close) => close.next_event().await,
        None => std::future::pending().await,
    }
}

impl<H: AgentRuntimeHost> AgentLoop<H> {
    pub(super) async fn finish_close_for_shutdown(&mut self) -> AgentRuntimeResult<AgentSnapshot> {
        // A new shutdown request is an explicit retry, just like a new close request.
        // Reset the recorded failure once, preserving the original physical lease.
        if let AgentState::Closing(state) = &self.state.snapshot.state {
            self.close(state.workspace_disposition()).await?;
        }
        if self.active.is_some() {
            self.interrupt_active_turn(pl_protocol::TurnCancellationCause::AgentClosed)
                .await?;
        }
        self.drain_session_sources().await?;
        self.drain_session_tasks().await?;
        while self.closing.is_some() {
            if let AgentState::Closing(state) = &self.state.snapshot.state
                && let Some(error) = state.error()
            {
                return Err(AgentRuntimeError::Lifecycle(error.message.clone()));
            }
            if let Some(child) = self
                .runtime
                .directory_snapshot()
                .agents
                .iter()
                .find(|agent| {
                    agent.identity.parent_id.as_ref() == Some(&self.state.snapshot.identity.id)
                        && !matches!(agent.state, AgentState::Closed(_))
                })
            {
                return Err(AgentRuntimeError::Lifecycle(format!(
                    "child {} has not completed cleanup",
                    child.identity.id
                )));
            }
            self.advance_session_close();
            let event = next_close_event(&mut self.closing).await;
            self.handle_close_event(event).await?;
        }
        Ok(self.state.snapshot.clone())
    }

    pub(super) async fn close(
        &mut self,
        disposition: pl_protocol::AgentWorkspaceDisposition,
    ) -> AgentRuntimeResult<AgentSnapshot> {
        if let AgentState::Closing(state) = &self.state.snapshot.state
            && state.workspace_disposition() != disposition
        {
            return Err(AgentRuntimeError::InvalidInput(
                "persisted cleanup disposition cannot change while closing".into(),
            ));
        }
        if matches!(self.state.snapshot.state, AgentState::Closed(_))
            && disposition == pl_protocol::AgentWorkspaceDisposition::Preserve
        {
            return Ok(self.state.snapshot.clone());
        }
        if let Some(close) = &self.closing {
            if close.disposition != disposition {
                return Err(AgentRuntimeError::InvalidInput(
                    "cleanup disposition cannot change while closing".into(),
                ));
            }
            if !matches!(close.phase, ClosePhase::Failed(_))
                && matches!(&self.state.snapshot.state, AgentState::Closing(state) if state.error().is_none())
            {
                return Ok(self.state.snapshot.clone());
            }
        }
        let mut next = self.state.clone();
        if !matches!(next.snapshot.state, AgentState::Closing(_)) {
            next.snapshot
                .transition(AgentCommand::BeginClose)
                .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
        }
        next.snapshot.state = AgentState::Closing(
            pl_protocol::ClosingAgentState::new()
                .with_disposition(disposition)
                .with_turn(self.active.as_ref().map(|active| active.turn_id.clone())),
        );
        next.pending_inputs.clear();
        next.refresh_mailbox_snapshot();
        next.session.inbox.close();
        self.commit_transition(
            super::persist::TransitionCommit::new(next).settlement(),
            |snapshot| AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(snapshot),
            },
        )
        .await?;
        self.dispatch_enabled = false;
        self.session_runtime.cancel();
        if let Some(active) = &mut self.active {
            active.request_cancellation(pl_protocol::TurnCancellationCause::AgentClosed);
            active.cancellation.cancel();
        }
        self.wake_session_waiter();
        if let Some(close) = &mut self.closing
            && !matches!(close.phase, ClosePhase::Failed(_))
        {
            // Drain failures do not surrender an in-flight preparation/cleanup future.
            close.cancel_deadline = Some(tokio::time::Instant::now() + self.cancel_grace);
            self.cancel_all_session_tasks().await?;
            return Ok(self.state.snapshot.clone());
        }
        let previous = self.closing.take();
        let retained = previous.and_then(|close| match close.phase {
            ClosePhase::Failed(lease) => lease,
            _ => None,
        });
        let phase = match retained {
            Some(lease) => ClosePhase::Draining(lease),
            None => {
                let host = self.host.clone();
                let request = CloseLifecycleRequest {
                    agent: self.state.snapshot.clone(),
                    workspace_disposition: disposition,
                };
                ClosePhase::Preparing(
                    async move {
                        std::panic::AssertUnwindSafe(host.lifecycle().prepare_close(request))
                            .catch_unwind()
                            .await
                            .map_err(|_| {
                                AgentRuntimeError::Lifecycle("close preparation panicked".into())
                            })?
                            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))
                    }
                    .boxed(),
                )
            }
        };
        self.closing = Some(SessionClose {
            disposition,
            phase,
            updates: self.runtime.subscribe_directory(),
            cancel_deadline: Some(tokio::time::Instant::now() + self.cancel_grace),
        });
        self.cancel_all_session_tasks().await?;
        Ok(self.state.snapshot.clone())
    }

    pub(super) fn advance_session_close(&mut self) {
        if self.state.session.tasks.active_ids().next().is_some()
            || self.active.is_some()
            || self.task_resources.has_work()
            || self.session_runtime.has_sources()
        {
            return;
        }
        if self
            .runtime
            .directory_snapshot()
            .agents
            .iter()
            .any(|agent| {
                agent.identity.parent_id.as_ref() == Some(&self.state.snapshot.identity.id)
                    && !matches!(agent.state, AgentState::Closed(_))
            })
        {
            return;
        }
        let Some(close) = self.closing.as_mut() else {
            return;
        };
        if !matches!(close.phase, ClosePhase::Draining(_)) {
            return;
        }
        let ClosePhase::Draining(lease) =
            std::mem::replace(&mut close.phase, ClosePhase::Failed(None))
        else {
            return;
        };
        let host = self.host.clone();
        close.phase = ClosePhase::Cleaning(
            async move {
                let result = std::panic::AssertUnwindSafe(host.lifecycle().commit_close(&lease))
                    .catch_unwind()
                    .await
                    .map_err(|_| AgentRuntimeError::Lifecycle("host cleanup panicked".into()))
                    .and_then(|result| {
                        result.map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))
                    });
                (lease, result)
            }
            .boxed(),
        );
    }

    pub(super) async fn handle_close_event(
        &mut self,
        event: CloseEvent<CloseLease<H>>,
    ) -> AgentRuntimeResult<()> {
        match event {
            CloseEvent::Wake => {}
            CloseEvent::DirectoryClosed => {
                if let Some(close) = &mut self.closing {
                    let previous = std::mem::replace(&mut close.phase, ClosePhase::Failed(None));
                    close.phase = match previous {
                        ClosePhase::Draining(lease) | ClosePhase::Failed(Some(lease)) => {
                            ClosePhase::Failed(Some(lease))
                        }
                        ClosePhase::Failed(None) => ClosePhase::Failed(None),
                        phase @ (ClosePhase::Preparing(_) | ClosePhase::Cleaning(_)) => phase,
                    };
                }
                self.record_close_error(AgentRuntimeError::Lifecycle(
                    "directory closed during session cleanup".into(),
                ))
                .await?;
            }
            CloseEvent::CancelDeadline => {
                if self.active.is_some() {
                    self.interrupt_active_turn(pl_protocol::TurnCancellationCause::AgentClosed)
                        .await?;
                }
            }
            CloseEvent::Prepared(Ok(lease)) => {
                if let Some(close) = &mut self.closing {
                    close.phase = ClosePhase::Draining(lease);
                }
            }
            CloseEvent::Prepared(Err(error)) => {
                if let Some(close) = &mut self.closing {
                    close.phase = ClosePhase::Failed(None);
                }
                self.record_close_error(error).await?;
            }
            CloseEvent::Cleaned(lease, Err(error)) => {
                if let Some(close) = &mut self.closing {
                    close.phase = ClosePhase::Failed(Some(lease));
                }
                self.record_close_error(error).await?;
            }
            CloseEvent::Cleaned(lease, Ok(())) => {
                if let Some(close) = &mut self.closing {
                    close.phase = ClosePhase::Failed(Some(lease));
                }
                let mut next = self.state.clone();
                next.active_input = None;
                next.pending_inputs.clear();
                next.refresh_mailbox_snapshot();
                next.snapshot
                    .transition(AgentCommand::Close)
                    .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
                self.commit_transition(
                    super::persist::TransitionCommit::new(next).settlement(),
                    |snapshot| AgentRuntimeEventKind::StateChanged {
                        snapshot: Box::new(snapshot),
                    },
                )
                .await?;
                self.session_runtime.release_tools();
                self.closing = None;
            }
        }
        Ok(())
    }

    pub(super) async fn record_close_error(
        &mut self,
        error: AgentRuntimeError,
    ) -> AgentRuntimeResult<()> {
        let mut next = self.state.clone();
        if let AgentState::Closing(state) = &mut next.snapshot.state {
            *state = state.clone().with_error(pl_protocol::StateError {
                code: "sessionCleanupFailed".into(),
                message: error.to_string().chars().take(4096).collect(),
                retryable: true,
            });
        }
        self.commit_transition(
            super::persist::TransitionCommit::new(next).settlement(),
            |snapshot| AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(snapshot),
            },
        )
        .await
    }
}

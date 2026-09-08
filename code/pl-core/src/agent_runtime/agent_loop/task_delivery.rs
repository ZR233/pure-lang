use super::super::{
    AgentRuntimeError, AgentRuntimeHost, AgentRuntimeResult, ThreadActorState, TurnId,
};
use super::AgentLoop;
use crate::session_runtime::{
    SessionTaskSubmission, SessionWakeEvent, ToolTaskDelivery, ToolTaskSnapshot,
};

impl<H: AgentRuntimeHost> AgentLoop<H> {
    fn validate_task_turn(&self, turn_id: &TurnId) -> AgentRuntimeResult<()> {
        if !self.state.snapshot.state.is_operational()
            || self.active.as_ref().is_none_or(|active| {
                &active.turn_id != turn_id
                    || active.is_cancelling()
                    || active.cancellation.is_cancelled()
            })
        {
            return Err(AgentRuntimeError::InvalidInput(
                "tool delivery requires the active Turn".into(),
            ));
        }
        Ok(())
    }

    pub(super) async fn admit_tool_tasks(
        &mut self,
        turn_id: &TurnId,
        tasks: Vec<SessionTaskSubmission>,
    ) -> AgentRuntimeResult<tokio::time::Instant> {
        self.validate_task_turn(turn_id)?;
        self.flush_pending_traces().await?;
        let mut next = self.state.clone();
        let mut admitted = Vec::new();
        for task in tasks {
            if task.receipt.thread_id != self.state.snapshot.identity.id.as_str()
                || task.receipt.turn_id != turn_id.as_str()
            {
                return Err(delivery_error("tool task belongs to another Turn"));
            }
            if next
                .session
                .tasks
                .admit(
                    task.receipt.clone(),
                    task.arguments_hash.clone(),
                    super::super::state::unix_timestamp(),
                )
                .map_err(delivery_error)?
            {
                next.session
                    .inbox
                    .reserve(&task.receipt.task_id)
                    .map_err(delivery_error)?;
                admitted.push(task);
            }
        }
        let deadline = tokio::time::Instant::now()
            .checked_add(self.session_runtime.handle.delivery_window())
            .ok_or_else(|| delivery_error("task delivery deadline overflow"))?;
        self.commit_session_task_state(next).await?;
        for task in admitted {
            self.task_resources.enqueue(task);
        }
        // Admission is already committed; startup failure must not masquerade as rejection.
        if let Err(error) = self.start_session_tasks().await {
            self.fault(format!("accepted task scheduling failed: {error}"))
                .await;
        }
        Ok(deadline)
    }

    pub(super) async fn select_tool_task_results(
        &mut self,
        turn_id: &TurnId,
        ids: &[String],
        deadline: tokio::time::Instant,
    ) -> AgentRuntimeResult<Vec<ToolTaskSnapshot>> {
        self.validate_task_turn(turn_id)?;
        let mut next = self.state.clone();
        for id in ids {
            let task = next.session.tasks.get(id).map_err(delivery_error)?;
            if task.receipt.turn_id != turn_id.as_str() {
                return Err(delivery_error("task delivery Turn mismatch"));
            }
            if task.delivery == ToolTaskDelivery::PendingResponse {
                let delivery = if task.status.is_terminal()
                    && self.task_resources.completed_before(id, deadline)
                {
                    ToolTaskDelivery::DirectOffered
                } else {
                    ToolTaskDelivery::Background
                };
                next.session
                    .tasks
                    .set_delivery(id, delivery)
                    .map_err(delivery_error)?;
                let task = next.session.tasks.get(id).map_err(delivery_error)?.clone();
                if delivery == ToolTaskDelivery::Background && task.status.is_terminal() {
                    next.session
                        .inbox
                        .publish_reserved(
                            id,
                            "tools",
                            SessionWakeEvent::ToolFinished(task),
                            super::super::state::unix_timestamp(),
                        )
                        .map_err(delivery_error)?;
                }
            }
        }
        self.commit_session_task_state(next).await?;
        self.wake_session_waiter();
        let mut selected = Vec::with_capacity(ids.len());
        for id in ids {
            selected.push(self.read_complete_task(id).await?);
        }
        Ok(selected)
    }

    pub(super) fn commit_direct_results(
        next: &mut ThreadActorState,
        turn_id: &TurnId,
        ids: &[String],
    ) -> AgentRuntimeResult<()> {
        for id in ids {
            let task = next.session.tasks.get(id).map_err(delivery_error)?;
            if task.receipt.turn_id != turn_id.as_str()
                || task.delivery != ToolTaskDelivery::DirectOffered
                || !task.status.is_terminal()
            {
                return Err(delivery_error(
                    "direct result does not match the offered task",
                ));
            }
            next.session
                .tasks
                .set_delivery(id, ToolTaskDelivery::DirectCommitted)
                .map_err(delivery_error)?;
            next.session.inbox.release_reservation(id);
        }
        Ok(())
    }

    pub(super) async fn release_turn_deliveries(
        &mut self,
        turn_id: Option<&TurnId>,
    ) -> AgentRuntimeResult<()> {
        let ids: Vec<_> = self
            .state
            .session
            .tasks
            .records()
            .filter(|task| {
                turn_id.is_none_or(|turn| task.receipt.turn_id == turn.as_str())
                    && matches!(
                        task.delivery,
                        ToolTaskDelivery::PendingResponse | ToolTaskDelivery::DirectOffered
                    )
            })
            .map(|task| task.receipt.task_id.clone())
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        for id in ids {
            next.session
                .tasks
                .set_delivery(&id, ToolTaskDelivery::Background)
                .map_err(delivery_error)?;
            let task = next.session.tasks.get(&id).map_err(delivery_error)?.clone();
            if task.status.is_terminal() {
                next.session
                    .inbox
                    .publish_reserved(
                        &id,
                        "tools",
                        SessionWakeEvent::ToolFinished(task),
                        super::super::state::unix_timestamp(),
                    )
                    .map_err(delivery_error)?;
            }
        }
        self.commit_session_task_state(next).await?;
        self.wake_session_waiter();
        Ok(())
    }
}

fn delivery_error(error: impl std::fmt::Display) -> AgentRuntimeError {
    AgentRuntimeError::InvalidInput(error.to_string())
}

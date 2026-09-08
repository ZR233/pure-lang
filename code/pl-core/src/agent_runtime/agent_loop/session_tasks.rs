use crate::session_runtime::{
    SessionTaskCompletion, SessionWakeEvent, ToolTaskResult, ToolTaskSnapshot, ToolTaskStatus,
};

use super::super::{
    AgentRuntimeError, AgentRuntimeHost, AgentRuntimeResult, DurableCommitFacts, PersistenceClass,
    ThreadActorState, ThreadMutation,
};
use super::AgentLoop;
use super::commit::{CommitPublication, PendingCommit};

impl<H: AgentRuntimeHost> AgentLoop<H> {
    pub(super) async fn read_complete_task(
        &mut self,
        id: &str,
    ) -> AgentRuntimeResult<ToolTaskSnapshot> {
        use super::super::ThreadRepository;
        let mut task = self
            .state
            .session
            .tasks
            .get(id)
            .map_err(task_error)?
            .clone();
        if let Some(record) = self
            .state
            .session
            .tasks
            .complete_result(id)
            .map_err(task_error)?
        {
            let content = match &record.content {
                Some(content) => (**content).clone(),
                None => self
                    .host
                    .repository()
                    .read_tool_task_result(&self.state.snapshot.identity.id, id)
                    .await
                    .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?
                    .ok_or_else(|| task_error("complete task output is missing from storage"))?,
            };
            record.validate_content(&content).map_err(task_error)?;
            task.result = Some(content);
            task.result_reference = None;
        }
        Ok(task)
    }

    pub(super) async fn append_session_task_output(
        &mut self,
        id: &str,
        delta: &str,
    ) -> AgentRuntimeResult<()> {
        let task = self.state.session.tasks.get(id).map_err(task_error)?;
        if !matches!(
            task.status,
            ToolTaskStatus::Running | ToolTaskStatus::Cancelling
        ) {
            return Ok(());
        }
        let thread_id = self.state.snapshot.identity.id.clone();
        let current = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let item = crate::session_runtime::task_output_item(
            task,
            current
                .items
                .iter()
                .find(|item| item.id == task.receipt.item_id),
            delta,
        )
        .map_err(task_error)?;
        self.record_thread_facts(
            thread_id,
            vec![crate::ThreadNotificationFact::durable(
                item.updated_at,
                pl_protocol::ThreadNotification::ItemStarted {
                    item: Box::new(item),
                },
            )],
        )
        .await
    }

    pub(super) async fn recover_session_tasks(&mut self) -> AgentRuntimeResult<()> {
        self.release_turn_deliveries(None).await?;
        let active: Vec<_> = self
            .state
            .session
            .tasks
            .active_ids()
            .map(str::to_owned)
            .collect();
        let reserved: Vec<_> = self
            .state
            .session
            .inbox
            .reservations()
            .filter(|id| !id.starts_with("source:"))
            .map(str::to_owned)
            .collect();
        if active != reserved {
            return Err(task_error(
                "task state and terminal event reservations disagree",
            ));
        }
        if active.is_empty() {
            return Ok(());
        }
        let now = super::super::state::unix_timestamp();
        let mut next = self.state.clone();
        for id in active {
            let snapshot = next
                .session
                .tasks
                .finish(
                    &id,
                    ToolTaskStatus::Interrupted,
                    text_result("Runtime restarted; physical tool execution was not replayed."),
                    now,
                )
                .map_err(task_error)?;
            next.session
                .inbox
                .publish_reserved(&id, "tools", SessionWakeEvent::ToolFinished(snapshot), now)
                .map_err(task_error)?;
        }
        self.commit_session_task_state(next).await
    }

    pub(super) async fn start_session_tasks(&mut self) -> AgentRuntimeResult<()> {
        if !self.state.snapshot.state.is_operational() {
            return Ok(());
        }
        while let Some(task) = self.task_resources.next_ready() {
            let mut next = self.state.clone();
            next.session
                .tasks
                .transition(
                    &task.receipt.task_id,
                    task.start_phase.status(),
                    super::super::state::unix_timestamp(),
                )
                .map_err(task_error)?;
            self.commit_session_task_state(next).await?;
            self.task_resources.launch_next();
        }
        Ok(())
    }

    pub(super) async fn mark_session_task_running(&mut self, id: &str) -> AgentRuntimeResult<()> {
        let mut next = self.state.clone();
        next.session
            .tasks
            .transition(
                id,
                ToolTaskStatus::Running,
                super::super::state::unix_timestamp(),
            )
            .map_err(task_error)?;
        self.commit_session_task_state(next).await
    }

    pub(super) async fn finish_session_task(
        &mut self,
        completion: SessionTaskCompletion,
    ) -> AgentRuntimeResult<()> {
        let current = self
            .state
            .session
            .tasks
            .get(&completion.id)
            .map_err(task_error)?;
        let result = crate::session_runtime::task_result(&completion.output);
        let status = if completion.cancelled || current.status == ToolTaskStatus::Cancelling {
            ToolTaskStatus::Cancelled
        } else if completion.output.success
            && !completion.output.timed_out
            && !result
                .facts
                .iter()
                .any(crate::session_runtime::is_rejected_control)
            && !completion
                .output
                .runtime_events
                .iter()
                .any(|event| matches!(event, crate::ToolDirective::ExecutionFailed))
        {
            ToolTaskStatus::Succeeded
        } else {
            ToolTaskStatus::Failed
        };
        let now = super::super::state::unix_timestamp();
        let mut next = self.state.clone();
        let snapshot = next
            .session
            .tasks
            .finish(&completion.id, status, result, now)
            .map_err(task_error)?;
        if snapshot.delivery == crate::session_runtime::ToolTaskDelivery::Background {
            next.session
                .inbox
                .publish_reserved(
                    &completion.id,
                    "tools",
                    SessionWakeEvent::ToolFinished(snapshot),
                    now,
                )
                .map_err(task_error)?;
        }
        self.commit_session_task_state(next).await?;
        self.task_resources.acknowledge_completion(&completion.id);
        self.wake_session_waiter();
        Ok(())
    }

    pub(super) async fn cancel_session_task(
        &mut self,
        id: &str,
    ) -> AgentRuntimeResult<ToolTaskSnapshot> {
        let snapshot = self.state.session.tasks.get(id).map_err(task_error)?;
        if snapshot.status.is_terminal() {
            return Ok(snapshot.clone());
        }
        let queued = snapshot.status == ToolTaskStatus::Queued;
        let now = super::super::state::unix_timestamp();
        let mut next = self.state.clone();
        let result = if queued {
            let snapshot = next
                .session
                .tasks
                .finish(
                    id,
                    ToolTaskStatus::Cancelled,
                    text_result("Task cancelled before execution."),
                    now,
                )
                .map_err(task_error)?;
            if snapshot.delivery == crate::session_runtime::ToolTaskDelivery::Background {
                next.session
                    .inbox
                    .publish_reserved(
                        id,
                        "tools",
                        SessionWakeEvent::ToolFinished(snapshot.clone()),
                        now,
                    )
                    .map_err(task_error)?;
            }
            snapshot
        } else {
            next.session
                .tasks
                .transition(id, ToolTaskStatus::Cancelling, now)
                .map_err(task_error)?;
            next.session.tasks.get(id).map_err(task_error)?.clone()
        };
        self.commit_session_task_state(next).await?;
        self.task_resources.cancel(id);
        self.wake_session_waiter();
        Ok(result)
    }

    pub(super) async fn cancel_all_session_tasks(&mut self) -> AgentRuntimeResult<()> {
        let ids: Vec<_> = self
            .state
            .session
            .tasks
            .active_ids()
            .map(str::to_owned)
            .collect();
        for id in ids {
            self.cancel_session_task(&id).await?;
        }
        Ok(())
    }

    pub(super) async fn drain_session_tasks(&mut self) -> AgentRuntimeResult<()> {
        self.task_resources.resume_settlement();
        self.cancel_all_session_tasks().await?;
        let deadline = tokio::time::Instant::now() + self.cancel_grace;
        while self.task_resources.has_completions_or_workers() {
            let completion =
                tokio::time::timeout_at(deadline, self.task_resources.next_completion())
                    .await
                    .map_err(|_| {
                        AgentRuntimeError::Lifecycle(
                            "task cleanup is pending; resources remain owned".into(),
                        )
                    })?;
            if let Some(completion) = completion {
                self.finish_session_task(completion).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn commit_session_task_state(
        &mut self,
        mut next: ThreadActorState,
    ) -> AgentRuntimeResult<()> {
        next.snapshot.revision = next
            .snapshot
            .revision
            .checked_add(1)
            .ok_or_else(|| task_error("session revision exhausted"))?;
        next.snapshot.updated_at = super::super::state::unix_timestamp();
        let current = self
            .runtime
            .thread_events
            .snapshot(self.state.snapshot.identity.id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let changes = crate::session_runtime::task_notifications(
            &next.session.tasks,
            &self.state.session.tasks,
            &current,
        )
        .map_err(task_error)?;
        let mut publication =
            CommitPublication::new(Some(self.state.snapshot.identity.id.clone()), None)
                .store_directory_snapshot();
        let projection = if changes.is_empty() {
            None
        } else {
            let projected = crate::thread_event::project_thread_facts(
                self.state.snapshot.identity.id.as_str(),
                &current,
                changes,
            );
            let projected = self
                .runtime
                .thread_events
                .project(
                    self.state.snapshot.identity.id.as_str(),
                    &projected.notifications,
                )
                .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
            next.session.thread_revision = projected.snapshot.revision;
            publication = publication.with_thread_notifications(projected.notifications.clone());
            Some(super::super::ThreadProjectionCommit {
                snapshot: projected.snapshot,
                notifications: projected.notifications,
            })
        };
        let facts = DurableCommitFacts::from_state(&next, Vec::new(), Vec::new(), projection, None);
        self.commit_and_publish(
            PendingCommit::new(next, facts, ThreadMutation::SnapshotAndQueue)
                .persistence(PersistenceClass::Standard)
                .publish(publication),
        )
        .await?;
        self.session_runtime.tasks_changed.notify_waiters();
        Ok(())
    }
}

fn task_error(error: impl std::fmt::Display) -> AgentRuntimeError {
    AgentRuntimeError::InvalidInput(error.to_string())
}

fn text_result(output: &str) -> ToolTaskResult {
    ToolTaskResult {
        facts: Vec::new(),
        structured_content: None,
        skill_activations: Vec::new(),
        timed_out: false,
        output: output.to_owned(),
        exit_code: None,
        output_file: None,
        attachments: Vec::new(),
    }
}

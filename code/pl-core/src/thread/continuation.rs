//! Owner-controlled interruption followed by a fresh Turn; admission receipts are not completions.
use super::*;

impl Owner {
    pub(super) fn continue_input(
        &mut self,
        input: input::ThreadInput,
        options: input::InputDriverOptions,
    ) -> Result<input::InputRecord, ThreadError> {
        // Duplicate submissions must not revive a manually paused driver or cancel a newer Turn.
        if self
            .state
            .inputs
            .iter()
            .any(|record| record.input.id == input.id)
        {
            return self.accept_input(input);
        }
        self.validate_continuation_admission()?;
        let record = self.accept_input(input)?;
        self.input_batch_through = Some(record.ordinal);
        self.request_continuation(options);
        Ok(record)
    }

    pub(super) fn continue_message(
        &mut self,
        message: inbox::ThreadMessage,
        options: input::InputDriverOptions,
    ) -> Result<u64, ThreadError> {
        if let Some(record) = self
            .state
            .inbox
            .iter()
            .find(|record| record.message.id == message.id)
        {
            return if record.message == message {
                Ok(record.sequence)
            } else {
                Err(ThreadError::InvalidIdentity)
            };
        }
        self.validate_continuation_admission()?;
        let sequence = self.receive_message(message, Some(options))?;
        self.request_continuation(options);
        Ok(sequence)
    }

    fn validate_continuation_admission(&mut self) -> Result<(), ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if self
            .state
            .interactions
            .values()
            .any(|record| record.state == interactions::InteractionState::Pending)
            || self
                .state
                .permissions
                .values()
                .any(|record| record.state == permissions::PermissionState::Pending)
        {
            return Err(ThreadError::PendingInteraction);
        }
        self.refresh_storage_pressure();
        if self.state.persistence.pressure_paused {
            return Err(ThreadError::StoragePressure);
        }
        if let Some(error) = &self.cold_error {
            return Err(ThreadError::Storage(error.clone()));
        }
        Ok(())
    }

    fn request_continuation(&mut self, options: input::InputDriverOptions) {
        self.input_driver.enable(options);
        if let Some(turn) = self
            .state
            .turns
            .iter()
            .rev()
            .find(|turn| turn.state == TurnState::Running)
        {
            let turn_id = turn.turn_id.clone();
            self.interrupted_turn = Some(turn_id.clone());
            // The receipt remains successful after admission even if cancellation cannot be committed.
            if let Err(error) = self.cancel_turn_tasks(&turn_id) {
                self.input_driver.fail(Arc::new(error));
                self.pause_inputs();
            }
            self.interrupt.interrupt();
        }
        self.publish_snapshot();
    }

    pub(super) fn cancel_turn_tasks(&mut self, turn_id: &str) -> Result<(), ThreadError> {
        let tasks: Vec<_> = self
            .state
            .tasks
            .values()
            .filter(|task| task.turn_id == turn_id && task.status == task::TaskStatus::Running)
            .map(|task| task.id.clone())
            .collect();
        for task in tasks {
            self.cancel_task(&task)?;
        }
        Ok(())
    }

    pub(super) async fn settle_turn_cancellation(&mut self, turn_id: &str) {
        while self
            .state
            .tasks
            .values()
            .any(|task| task.turn_id == turn_id && task.status == task::TaskStatus::Running)
        {
            if self
                .uncommitted_tools
                .values()
                .any(|completion| completion.call.turn_id == turn_id)
            {
                self.input_driver
                    .fail(Arc::new(ThreadError::PendingToolCommit));
                self.pause_inputs();
                break;
            }
            tokio::select! {
                Some(completion) = futures::StreamExt::next(&mut self.background), if !self.background.is_empty() => self.finish_background(completion),
                message = self.mailbox.recv(), if !self.mailbox.is_closed() || !self.mailbox.is_empty() => {
                    if let Some(message) = message { self.process_mailbox(message); }
                }
            }
        }
    }

    /// Wait in the owner loop for physical task completion, servicing the mailbox throughout.
    pub(super) fn continuation_ready(&mut self) -> bool {
        let Some(turn_id) = &self.interrupted_turn else {
            return true;
        };
        if self
            .state
            .turns
            .iter()
            .any(|turn| &turn.turn_id == turn_id && turn.state == TurnState::Running)
            || self
                .state
                .tasks
                .values()
                .any(|task| &task.turn_id == turn_id && task.status == task::TaskStatus::Running)
        {
            if self
                .uncommitted_tools
                .values()
                .any(|completion| &completion.call.turn_id == turn_id)
            {
                self.input_driver
                    .fail(Arc::new(ThreadError::PendingToolCommit));
                self.pause_inputs();
            }
            return false;
        }
        self.interrupted_turn = None;
        self.publish_snapshot();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelSession, ModelToolCall, PreparedModelCall};
    use crate::tool::ToolOutput;
    use crate::tool::opaque::{CallContext, Registration, Tool, ToolError};
    use pretty_assertions::assert_eq;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    struct ToolModel(Arc<AtomicUsize>);
    impl ModelSession for ToolModel {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![],
                    tool_calls: if first {
                        vec![ModelToolCall {
                            call_id: "call".into(),
                            tool_id: "work".into(),
                            arguments: OpaquePayload::text("work"),
                        }]
                    } else {
                        vec![]
                    },
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    #[derive(Debug, Default)]
    struct Cleanup {
        started: Notify,
        release: Notify,
        token: Mutex<Option<CancellationToken>>,
        fail: bool,
    }
    impl Tool for Arc<Cleanup> {
        async fn execute(
            &self,
            _: OpaquePayload,
            context: CallContext,
        ) -> Result<ToolOutput, ToolError> {
            *self.token.lock().unwrap() = Some(context.cancellation.clone());
            self.started.notify_one();
            context.cancellation.cancelled().await;
            self.release.notified().await;
            if self.fail {
                return Err(ToolError::new(std::io::Error::other(
                    "physical cleanup failed",
                )));
            }
            Err(ToolError::new(ThreadError::Cancelled))
        }
    }
    fn options() -> input::InputDriverOptions {
        input::InputDriverOptions {
            max_model_steps: ModelStepLimit::Unlimited,
        }
    }
    fn prompt(id: &str) -> input::ThreadInput {
        input::ThreadInput {
            id: id.into(),
            payload: OpaquePayload::text(id),
            context: vec![ContextContent::Text { text: id.into() }],
        }
    }
    async fn tool_thread(
        foreground: bool,
        fail: bool,
    ) -> (ThreadHandle, Arc<Cleanup>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let cleanup = Arc::new(Cleanup {
            fail,
            ..Default::default()
        });
        let thread = ThreadHandle::start(
            "continuation".into(),
            DynModelSession::new(ToolModel(calls.clone())),
        )
        .unwrap();
        let registration =
            Registration::new("work".into(), OpaquePayload::text("work"), cleanup.clone()).unwrap();
        thread
            .register_tools(vec![if foreground {
                registration.foreground()
            } else {
                registration
            }])
            .await
            .unwrap();
        thread
            .submit_input_and_continue(prompt("first"), options())
            .await
            .unwrap();
        cleanup.started.notified().await;
        (thread, cleanup, calls)
    }
    async fn settled(thread: &ThreadHandle, turns: usize) -> ThreadSnapshot {
        let mut updates = thread.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let state = updates.next().await.unwrap();
                if state.turns.len() == turns
                    && state
                        .turns
                        .last()
                        .is_some_and(|turn| turn.state != TurnState::Running)
                    && !matches!(
                        state.input_execution,
                        input::InputExecution::Interrupting { .. }
                    )
                {
                    return state;
                }
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn redirect_cancels_foreground_and_background_tools_and_waits_for_physical_cleanup() {
        for foreground in [false, true] {
            let (thread, cleanup, calls) = tool_thread(foreground, false).await;
            let original = thread.snapshot().turns[0].turn_id.clone();
            thread
                .submit_input_and_continue(prompt("second"), options())
                .await
                .unwrap();
            assert!(
                cleanup
                    .token
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .is_cancelled()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(
                matches!(thread.snapshot().input_execution, input::InputExecution::Interrupting { turn_id } if turn_id == original)
            );
            assert_eq!(
                thread.snapshot().tasks["task:call"].status,
                task::TaskStatus::Running
            );
            cleanup.release.notify_one();
            let state = settled(&thread, 2).await;
            assert_eq!(state.tasks["task:call"].status, task::TaskStatus::Cancelled);
            assert_eq!(state.turns[0].state, TurnState::Interrupted);
            assert_eq!(
                state.turns[1].state,
                TurnState::Finished(TurnOutcome::Completed)
            );
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            let next = &state.attempts.last().unwrap().input;
            assert_eq!(
                next.records
                    .iter()
                    .flat_map(|record| &record.content)
                    .filter(|content| **content == prompt("first").context[0])
                    .count(),
                1
            );
            let history = thread.journal().await.unwrap();
            journal::replay(&history).unwrap();
            thread.close().await.unwrap();
        }
    }

    #[tokio::test]
    async fn manual_stop_and_cleanup_failure_prevent_automatic_continuation_without_losing_input() {
        for fail in [false, true] {
            let (thread, cleanup, calls) = tool_thread(false, fail).await;
            thread
                .submit_input_and_continue(prompt("second"), options())
                .await
                .unwrap();
            if !fail {
                let turn_id = thread.snapshot().turns[0].turn_id.clone();
                thread.interrupt_turn(Some(turn_id)).await.unwrap();
            }
            cleanup.release.notify_one();
            let state = settled(&thread, 1).await;
            assert_eq!(state.inputs[1].state, input::InputState::Pending);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            if fail {
                let input::InputExecution::Failed { error } = &state.input_execution else {
                    panic!("cleanup error must be visible")
                };
                assert!(error.to_string().contains("physical cleanup failed"));
            }
            thread
                .send_message_and_resume(
                    inbox::ThreadMessage {
                        id: "late-notification".into(),
                        source_id: "agent:child".into(),
                        payload: OpaquePayload::text("progress"),
                        context: vec![],
                    },
                    options(),
                )
                .await
                .unwrap();
            assert!(matches!(
                thread.snapshot().input_execution,
                input::InputExecution::Paused | input::InputExecution::Failed { .. }
            ));
            // A duplicate receipt must not undo Stop or retry a failed cleanup.
            thread
                .submit_input_and_continue(prompt("second"), options())
                .await
                .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            let history = thread.journal().await.unwrap();
            let restored = ThreadHandle::restore(
                "continuation".into(),
                DynModelSession::new(ToolModel(calls.clone())),
                history,
            )
            .unwrap();
            assert!(matches!(
                restored.snapshot().input_execution,
                input::InputExecution::Paused
            ));
            assert_eq!(
                restored.snapshot().inputs[1].state,
                input::InputState::Pending
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            restored.close().await.unwrap();
            thread.close().await.unwrap();
        }
    }

    #[tokio::test]
    async fn ordinary_notifications_do_not_interrupt_but_parent_prompts_do_and_are_idempotent() {
        let (thread, cleanup, calls) = tool_thread(true, false).await;
        let message = |id: &str| inbox::ThreadMessage {
            id: id.into(),
            source_id: "agent:parent".into(),
            payload: OpaquePayload::text(id),
            context: vec![ContextContent::Text { text: id.into() }],
        };
        thread.send_message(message("progress")).await.unwrap();
        assert!(
            !cleanup
                .token
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled()
        );
        let sequence = thread
            .send_message_and_continue(message("redirect"), options())
            .await
            .unwrap();
        assert!(
            cleanup
                .token
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled()
        );
        assert_eq!(
            thread
                .send_message_and_continue(message("redirect"), options())
                .await
                .unwrap(),
            sequence
        );
        cleanup.release.notify_one();
        let state = settled(&thread, 2).await;
        assert_eq!(state.consumed_messages, sequence);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        thread
            .send_message_and_continue(message("redirect"), options())
            .await
            .unwrap();
        assert_eq!(thread.snapshot().turns.len(), 2);
        thread.close().await.unwrap();
    }
}

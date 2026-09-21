//! Owner-controlled interruption followed by a fresh Turn; admission receipts are not completions.
use super::*;

impl Owner {
    pub(super) fn continue_input(
        &mut self,
        input: input::ThreadInput,
        options: input::InputDriverOptions,
    ) -> Result<input::InputRecord, ThreadError> {
        // Duplicate submissions must not revive a manually paused driver or cancel a newer Turn.
        // Terminal identities stay resident even after their input body is pruned, so the receipt
        // is answered from bounded state instead of reviving a paused driver.
        if let Some(receipt) = input::duplicate_input_receipt(&self.state, &input)? {
            return Ok(receipt);
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
        // An already accepted identity is answered from its original receipt before any new
        // admission check or continuation action: a repeat is not new work, so it must not be
        // refused because the owner is closing, is waiting for an interaction or is under storage
        // pressure, and it must not interrupt the Turn that is currently running. Only a genuinely
        // new message takes the admission path, which is what warms the driver and interrupts.
        if let Some(sequence) = self.accepted_message_receipt(&message)? {
            return Ok(sequence);
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
    use crate::thread::tests::history_snapshot;
    use crate::tool::ToolOutput;
    use crate::tool::opaque::{CallContext, Registration, Tool, ToolError};
    use pretty_assertions::assert_eq;
    use std::sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
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
    /// Answers every model call with the same tool request, so a Turn can be kept physically running
    /// until its own cancellation is requested.
    struct AlwaysToolModel(Arc<AtomicUsize>);
    impl ModelSession for AlwaysToolModel {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let index = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![],
                    tool_calls: vec![ModelToolCall {
                        call_id: format!("call-{index}"),
                        tool_id: "work".into(),
                        arguments: OpaquePayload::text("work"),
                    }],
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    /// A model that answers immediately without a tool call: one Turn consumes its inbox message and
    /// commits a terminal Turn, leaving no physical resource behind.
    struct Reply;
    impl ModelSession for Reply {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: Arc::from("response"),
                    }],
                    tool_calls: vec![],
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    /// Reports a fixed Thread byte budget, so admission pressure is deterministic without IO.
    #[derive(Debug)]
    struct PressureStore(Arc<AtomicU64>);
    impl cold::ColdStore for PressureStore {
        fn pressure(&self, _: &str) -> cold::StoragePressure {
            let bytes = self.0.load(Ordering::SeqCst);
            cold::StoragePressure {
                thread_bytes: bytes,
                store_bytes: bytes,
                error: None,
            }
        }
        fn admit(&self, _: &str, _: cold::ThreadWrite) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
        async fn flush(&self, _: &str, _: u64) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
    }
    /// A parent-authored message with a frozen body, so a repeat can be byte-identical.
    fn parent_message(id: &str) -> inbox::ThreadMessage {
        inbox::ThreadMessage {
            id: id.into(),
            source_id: "agent:parent".into(),
            payload: OpaquePayload::text(id),
            context: vec![ContextContent::Text { text: id.into() }],
        }
    }
    /// A Thread whose first Turn holds a physically running foreground tool call.
    async fn always_running_thread() -> (ThreadHandle, Arc<Cleanup>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let cleanup = Arc::new(Cleanup::default());
        let thread = ThreadHandle::start(
            "continuation".into(),
            DynModelSession::new(AlwaysToolModel(calls.clone())),
        )
        .unwrap();
        let registration =
            Registration::new("work".into(), OpaquePayload::text("work"), cleanup.clone()).unwrap();
        thread
            .register_tools(vec![registration.foreground()])
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
                updates.next().await.unwrap();
                // Terminal Turns are history facts, so settle on the committed projection while
                // live driver state (input execution) still comes from the snapshot.
                let state = history_snapshot(thread).await;
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
            let checkpoint = thread
                .checkpoint(thread.snapshot().commit_sequence)
                .unwrap();
            let restored = ThreadHandle::resume(
                "continuation".into(),
                DynModelSession::new(ToolModel(calls.clone())),
                Some(checkpoint),
            )
            .unwrap();
            assert!(matches!(
                restored.snapshot().input_execution,
                input::InputExecution::Paused
            ));
            assert!(
                restored
                    .snapshot()
                    .inputs
                    .iter()
                    .all(|record| record.state == input::InputState::Pending),
                "a resumed owner keeps only pending queued inputs"
            );
            assert!(
                restored
                    .snapshot()
                    .inputs
                    .iter()
                    .any(|record| record.input.id == "second"),
                "the queued input stays pending after an interrupted Turn"
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
        assert_eq!(history_snapshot(&thread).await.turns.len(), 2);
        thread.close().await.unwrap();
    }

    /// A repeat of an identity the owner already accepted must never be mistaken for new work: it
    /// keeps its original receipt and leaves the Turn that is currently running untouched.
    #[tokio::test]
    async fn repeated_consumed_identity_keeps_its_receipt_and_never_interrupts_running_work() {
        let (thread, cleanup, calls) = always_running_thread().await;
        // A brand-new identity is admitted and interrupts the running Turn, as before.
        let sequence = thread
            .send_message_and_continue(parent_message("redirect"), options())
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
        cleanup.release.notify_one();
        // The redirect Turn is now running its own physical call and already consumed the message.
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            cleanup.started.notified(),
        )
        .await
        .expect("the redirect Turn starts its own tool");
        let running = thread.snapshot();
        assert_eq!(running.consumed_messages, sequence);
        assert!(
            !cleanup
                .token
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled()
        );
        let committed = history_snapshot(&thread).await.turns.len();
        // The repeat is answered from the original receipt: no second message, no new Turn and no
        // interruption of the work that is still running.
        assert_eq!(
            thread
                .send_message_and_continue(parent_message("redirect"), options())
                .await
                .unwrap(),
            sequence
        );
        assert!(
            !cleanup
                .token
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled(),
            "a repeated identity must not interrupt the running Turn"
        );
        assert_eq!(thread.snapshot().consumed_messages, sequence);
        assert_eq!(history_snapshot(&thread).await.turns.len(), committed);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        // The same identity carrying a different body stays an identity conflict and is still not
        // new work: it creates no second message and does not disturb the running Turn.
        let mut conflicting = parent_message("redirect");
        conflicting.payload = OpaquePayload::text("different body");
        assert!(matches!(
            thread
                .send_message_and_continue(conflicting, options())
                .await,
            Err(ThreadError::InvalidIdentity)
        ));
        assert!(
            !cleanup
                .token
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        // Teardown: cancel the running Turn so its physical cleanup can finish.
        let turn_id = thread.snapshot().turns[0].turn_id.clone();
        thread.interrupt_turn(Some(turn_id)).await.unwrap();
        cleanup.release.notify_one();
        let _ = settled(&thread, 2).await;
        thread.close().await.unwrap();
    }

    /// A repeat of an already consumed identity keeps its receipt even when a brand-new message
    /// would be refused right now: uniqueness is answered from the accepted identity, not re-judged
    /// against current admission pressure. A new identity still takes the normal admission checks.
    #[tokio::test]
    async fn repeated_consumed_identity_keeps_its_receipt_while_admission_is_pressured() {
        let thread =
            ThreadHandle::start("continuation".into(), DynModelSession::new(Reply)).unwrap();
        let sequence = thread
            .send_message_and_continue(parent_message("redirect"), options())
            .await
            .unwrap();
        let state = settled(&thread, 1).await;
        assert_eq!(state.consumed_messages, sequence);
        assert!(
            thread.snapshot().inbox.is_empty(),
            "a consumed message leaves the resident queue"
        );

        let bytes = Arc::new(AtomicU64::new(64 * 1024 * 1024));
        thread
            .attach_storage(cold::ColdStoreHandle::new(PressureStore(bytes.clone())))
            .await
            .unwrap();
        assert!(matches!(
            thread
                .send_message_and_continue(parent_message("brand-new"), options())
                .await,
            Err(ThreadError::StoragePressure)
        ));
        assert_eq!(
            thread
                .send_message_and_continue(parent_message("redirect"), options())
                .await
                .unwrap(),
            sequence
        );
        assert_eq!(thread.snapshot().consumed_messages, sequence);
        assert_eq!(history_snapshot(&thread).await.turns.len(), 1);
        let mut conflicting = parent_message("redirect");
        conflicting.payload = OpaquePayload::text("changed body");
        assert!(matches!(
            thread
                .send_message_and_continue(conflicting, options())
                .await,
            Err(ThreadError::InvalidIdentity)
        ));
        bytes.store(0, Ordering::SeqCst);
        thread.close().await.unwrap();
    }
}

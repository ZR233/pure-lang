//! Bounded owner commands that cannot modify an admitted model input.
use super::*;

#[derive(Debug)]
pub(super) enum MailboxCommand {
    ContinueInput(
        input::ThreadInput,
        input::InputDriverOptions,
        oneshot::Sender<Result<input::InputRecord, ThreadError>>,
    ),
    ContinueMessage(
        inbox::ThreadMessage,
        input::InputDriverOptions,
        oneshot::Sender<Result<u64, ThreadError>>,
    ),
    QueueRuntimeFacts(Vec<RuntimeFact>, oneshot::Sender<Result<(), ThreadError>>),
    /// One accepted increment of a running tool call's live output.
    ///
    /// The command carries only what changed — one chunk, or the whole bounded window after the
    /// producer rolled over — so a progress report never ships the accumulated text back to the
    /// owner.
    ToolProgress {
        caller: String,
        executor: crate::tool::opaque::ExecutionAuthority,
        update: crate::model::ToolProgressUpdate,
        reply: oneshot::Sender<Result<(), ThreadError>>,
    },
    /// A running tool's accepted output could not be stored; latch the typed fault now.
    ///
    /// The producer reports this through the same reliable, bounded channel it already uses for
    /// progress, so a capture write or archive that failed stops other model/tool admission before
    /// the call returns instead of only when `execute` unwinds. The fault keeps its
    /// [`crate::thread::cold::OutputRetryObligation`], so the pause stays until that exact obligation
    /// is retried and its bytes really stored.
    ToolOutputStorageFault {
        caller: String,
        executor: crate::tool::opaque::ExecutionAuthority,
        fault: Arc<cold::OutputStorageFault>,
        reply: oneshot::Sender<Result<(), ThreadError>>,
    },
    QueueModelUpdate(
        super::model_update::DeferredModelUpdate,
        super::model_update::DeferredModelUpdatePrecondition,
        Vec<extensions::ExtensionMutation>,
        oneshot::Sender<Result<ThreadSnapshot, ThreadError>>,
    ),
    BeginIdleClose(oneshot::Sender<bool>),
    Reconfigure(
        IdleReconfiguration,
        crate::tool::opaque::RegistrationBatch,
        oneshot::Sender<Result<ThreadSnapshot, ThreadError>>,
    ),
    InterruptTurn {
        expected: Option<String>,
        reply: oneshot::Sender<Result<bool, ThreadError>>,
    },
    SubmitInput(
        input::InputSubmission,
        oneshot::Sender<Result<input::InputRecord, ThreadError>>,
    ),
    RequestPermission {
        caller: String,
        authority: crate::tool::opaque::ExecutionAuthority,
        payload: OpaquePayload,
        reply: oneshot::Sender<Result<permissions::PermissionRecord, ThreadError>>,
    },
    ResolvePermission(
        permissions::PermissionResolution,
        oneshot::Sender<Result<permissions::PermissionRecord, ThreadError>>,
    ),
    ValidateExecution {
        caller: String,
        executor: crate::tool::opaque::ExecutionAuthority,
        reply: oneshot::Sender<Result<(), ThreadError>>,
    },
    ResumeInputs(
        input::InputDriverOptions,
        oneshot::Sender<Result<(), ThreadError>>,
    ),
    PauseInputs(oneshot::Sender<()>),
    Input(
        input::ThreadInput,
        Option<input::InputDriverOptions>,
        oneshot::Sender<Result<input::InputRecord, ThreadError>>,
    ),
    DiscardInput(
        String,
        oneshot::Sender<Result<input::InputRecord, ThreadError>>,
    ),
    RetryToolCommits(oneshot::Sender<Result<(), ThreadError>>),
    /// Continues new model/tool admission after a hard storage fault was recovered.
    ///
    /// The explicit resume must reach an owner that is already *inside* a running Turn: a paused
    /// Turn waits at a storage safety point (`await_storage_admission`) and only services this
    /// mailbox, so a resume that only travelled the outer command channel could never release the
    /// pause it was sent to release. Routing it here keeps one path for both idle and in-flight
    /// owners instead of two ladders with different reachability.
    ResumeStorage {
        generation: u64,
        reply: oneshot::Sender<Result<(), ThreadError>>,
    },
    /// Re-runs the accepted output the current fault owes and clears the obligation on success.
    ///
    /// Like the resume control, it must reach an owner that is already inside a running or paused
    /// Turn, so it travels this mailbox. It only clears the obligation; the generation, backend
    /// verdict and durability fence are still verified by the explicit resume, so this is an
    /// idempotent retry rather than a release.
    RetryOutputStorage(oneshot::Sender<Result<(), ThreadError>>),
    /// Makes every already-published fact durable, keeping the target fixed at request time.
    ///
    /// An explicit flush must reach an owner that is already *inside* a running or paused Turn, so
    /// it travels this mailbox like the resume control instead of the outer command channel: a
    /// paused Turn only services this mailbox, and a flush waiting on the outer channel there would
    /// wait for a Turn that cannot end on its own. The target is the admitted watermark this owner
    /// has when it handles the request, so a later admission never extends the barrier.
    Flush(oneshot::Sender<Result<(), ThreadError>>),
    ToolCancelTask {
        caller: String,
        authorization: Option<crate::tool::opaque::ToolAuthorization>,
        target: String,
        reply: oneshot::Sender<Result<task::TaskCancellationReceipt, ThreadError>>,
    },
    CancelTask(
        String,
        oneshot::Sender<Result<task::TaskCancellationReceipt, ThreadError>>,
    ),
    Register(
        Option<u64>,
        crate::tool::opaque::RegistrationBatch,
        oneshot::Sender<Result<(), ThreadError>>,
    ),
    Message(
        inbox::ThreadMessage,
        Option<input::InputDriverOptions>,
        oneshot::Sender<Result<u64, ThreadError>>,
    ),
    WakeAcceptedMessage {
        id: String,
        sequence: u64,
        options: input::InputDriverOptions,
        reply: oneshot::Sender<Result<bool, ThreadError>>,
    },
}

impl Owner {
    pub(super) fn process_mailbox(&mut self, command: MailboxCommand) {
        match command {
            MailboxCommand::ContinueInput(input, options, reply) => {
                let _ = reply.send(self.continue_input(input, options));
            }
            MailboxCommand::ContinueMessage(message, options, reply) => {
                let _ = reply.send(self.continue_message(message, options));
            }
            MailboxCommand::QueueRuntimeFacts(facts, reply) => {
                let _ = reply.send(self.queue_runtime_facts(facts));
            }
            MailboxCommand::ToolProgress {
                caller,
                executor,
                update,
                reply,
            } => {
                let result = self.report_tool_progress(caller, executor, update);
                let _ = reply.send(result);
            }
            MailboxCommand::ToolOutputStorageFault {
                caller,
                executor,
                fault,
                reply,
            } => {
                let result = self.report_tool_output_storage_fault(caller, executor, fault);
                let _ = reply.send(result);
            }
            MailboxCommand::QueueModelUpdate(update, precondition, mutations, reply) => {
                let result = self.queue_model_update(update, precondition, mutations);
                let _ = reply.send(result);
            }
            MailboxCommand::BeginIdleClose(reply) => {
                let idle = self.active_inputs.is_empty()
                    && !self
                        .state
                        .turns
                        .iter()
                        .any(|turn| turn.state == TurnState::Running)
                    && !self
                        .state
                        .inputs
                        .iter()
                        .any(|record| record.state == input::InputState::Pending)
                    && !self
                        .state
                        .tasks
                        .values()
                        .any(|task| task.status == task::TaskStatus::Running)
                    && !self
                        .state
                        .interactions
                        .values()
                        .any(|record| record.state == interactions::InteractionState::Pending)
                    && !self
                        .state
                        .permissions
                        .values()
                        .any(|record| record.state == permissions::PermissionState::Pending)
                    && !self
                        .state
                        .inbox
                        .iter()
                        .any(|record| record.sequence > self.state.consumed_messages)
                    && self.pending.is_empty()
                    && self.uncommitted_tools.is_empty()
                    && self.pending_effects.is_empty()
                    && self.cold_error.is_none()
                    && !self.cold.as_ref().is_some_and(|store| {
                        let pressure = store.pressure(&self.id);
                        pressure.thread_bytes > 0 || pressure.error.is_some()
                    });
                if idle {
                    self.interrupt.begin_close();
                    self.state.lifecycle = ThreadLifecycle::Closing;
                    self.publish();
                }
                let _ = reply.send(idle);
            }

            MailboxCommand::Reconfigure(mut update, batch, reply) => {
                let result = if self.state.lifecycle == ThreadLifecycle::Open
                    && !self.interrupt.is_closing()
                {
                    update.tools = batch.take().unwrap_or_default();
                    self.reconfigure(update)
                } else {
                    Err(ThreadError::Closed)
                };
                let _ = reply.send(result);
            }

            MailboxCommand::InterruptTurn { expected, reply } => {
                let active = self
                    .state
                    .turns
                    .iter()
                    .rev()
                    .find(|turn| turn.state == TurnState::Running)
                    .map(|turn| turn.turn_id.clone())
                    .or_else(|| self.interrupted_turn.clone());
                let result = match active {
                    Some(turn_id) if expected.as_ref().is_some_and(|id| id != &turn_id) => {
                        Err(ThreadError::InvalidIdentity)
                    }
                    Some(turn_id) => {
                        self.interrupted_turn = Some(turn_id.clone());
                        self.pause_inputs();
                        let result = self.cancel_turn_tasks(&turn_id);
                        let interrupted = self.interrupt.interrupt();
                        self.publish_snapshot();
                        result.map(|()| interrupted)
                    }
                    None => {
                        self.pause_inputs();
                        Ok(false)
                    }
                };
                let _ = reply.send(result);
            }

            MailboxCommand::SubmitInput(submission, reply) => {
                let result = self.accept_input_with_policy(submission.input, submission.policy);
                if result
                    .as_ref()
                    .is_ok_and(|record| record.state == input::InputState::Pending)
                    && let Some(options) = submission.drive
                    && let Err(error) = self.resume_inputs(options)
                {
                    self.input_driver.fail(Arc::new(error));
                    self.publish_snapshot();
                }
                let _ = reply.send(result);
            }
            MailboxCommand::RequestPermission {
                caller,
                authority,
                payload,
                reply,
            } => {
                let _ = reply.send(self.request_execution_permission(caller, authority, payload));
            }
            MailboxCommand::ResolvePermission(resolution, reply) => {
                let _ = reply.send(self.resolve_execution_permission(resolution));
            }
            MailboxCommand::ValidateExecution {
                caller,
                executor,
                reply,
            } => {
                let result = if self.state.lifecycle != ThreadLifecycle::Open
                    || self.interrupt.is_closing()
                {
                    Err(ThreadError::Closed)
                } else if !self
                    .state
                    .tasks
                    .get(&caller)
                    .is_some_and(|task| task.status == task::TaskStatus::Running)
                    || self
                        .task_tokens
                        .get(&caller)
                        .is_some_and(CancellationToken::is_cancelled)
                {
                    Err(ThreadError::TaskAccessExpired)
                } else if !executor.remains_authorized(&self.tools) {
                    Err(ThreadError::ToolPermissionRevoked)
                } else {
                    Ok(())
                };
                let _ = reply.send(result);
            }
            MailboxCommand::ResumeInputs(options, reply) => {
                let _ = reply.send(self.resume_inputs(options));
            }
            MailboxCommand::PauseInputs(reply) => {
                self.pause_inputs();
                let _ = reply.send(());
            }
            MailboxCommand::Input(input, drive, reply) => {
                let result = self.accept_input(input);
                if result
                    .as_ref()
                    .is_ok_and(|record| record.state == input::InputState::Pending)
                    && let Some(options) = drive
                    && let Err(error) = self.resume_inputs(options)
                {
                    // Admission is already committed: report its receipt even if close raced the drive request.
                    self.input_driver.fail(Arc::new(error));
                    self.publish_snapshot();
                }
                let _ = reply.send(result);
            }
            MailboxCommand::DiscardInput(id, reply) => {
                let _ = reply.send(self.discard_input(&id));
            }
            MailboxCommand::RetryToolCommits(reply) => {
                let _ = reply.send(self.retry_tool_commits());
            }
            MailboxCommand::ResumeStorage { generation, reply } => {
                let result = if self.interrupt.is_closing() {
                    Err(ThreadError::Closed)
                } else {
                    self.resume_storage(generation)
                };
                let _ = reply.send(result);
            }
            MailboxCommand::ToolCancelTask {
                caller,
                authorization,
                target,
                reply,
            } => {
                let result = match self.state.tasks.get(&caller) {
                    Some(task) if task.status == task::TaskStatus::Running => {
                        if self
                            .tools
                            .permits_task_cancellation(&task.tool_id, authorization.as_ref())
                        {
                            self.cancel_task(&target)
                        } else {
                            Err(ThreadError::TaskAccessDenied)
                        }
                    }
                    Some(_) | None => Err(ThreadError::TaskAccessExpired),
                };
                let _ = reply.send(result);
            }
            MailboxCommand::CancelTask(id, reply) => {
                let _ = reply.send(self.cancel_task(&id));
            }
            MailboxCommand::Register(expected, tools, reply) => {
                let result = if self.state.lifecycle == ThreadLifecycle::Open
                    && !self.interrupt.is_closing()
                {
                    if let Some(expected) = expected
                        && expected != self.state.extension_sequence
                    {
                        let _ = reply.send(Err(ThreadError::ExtensionSequenceConflict {
                            expected,
                            actual: self.state.extension_sequence,
                        }));
                        return;
                    }
                    let result = self
                        .tools
                        .replace(tools.take().unwrap_or_default())
                        .map_err(ThreadError::from)
                        .and_then(|()| self.revoke_stale_permissions());
                    if result.is_ok() {
                        self.state.discovered_tools = self.tools.discovery();
                        self.publish();
                    }
                    result
                } else {
                    Err(ThreadError::Closed)
                };
                let _ = reply.send(result);
            }
            MailboxCommand::Message(message, drive, reply) => {
                let result = if self.interrupt.is_closing() {
                    Err(ThreadError::Closed)
                } else {
                    self.receive_message(message, drive)
                };
                let _ = reply.send(result);
            }
            MailboxCommand::WakeAcceptedMessage {
                id,
                sequence,
                options,
                reply,
            } => {
                let _ = reply.send(self.wake_accepted_message(&id, sequence, options));
            }
            // Handled by `dispatch_mailbox`, which is the only place that may await the machine.
            MailboxCommand::Flush(reply) | MailboxCommand::RetryOutputStorage(reply) => {
                let _ = reply.send(Err(ThreadError::Closed));
            }
        }
    }

    /// Services one mailbox command, awaiting the ones that need the machine.
    ///
    /// An explicit flush is the only command that has to run the (async) durability barrier, and it
    /// must be reachable from inside a running or paused Turn — the very place a sync handler cannot
    /// await. Everything else stays synchronous, so the common path costs no extra state machine.
    pub(super) async fn dispatch_mailbox(&mut self, command: MailboxCommand) {
        match command {
            MailboxCommand::Flush(reply) => {
                let result = self.flush_cold().await;
                let _ = reply.send(result);
            }
            MailboxCommand::RetryOutputStorage(reply) => {
                let result = self.retry_output_obligation().await;
                let _ = reply.send(result);
            }
            other => self.process_mailbox(other),
        }
    }

    pub(super) async fn await_with_mailbox<T>(
        &mut self,
        operation: impl std::future::Future<Output = T> + Send,
    ) -> T {
        tokio::pin!(operation);
        loop {
            tokio::select! {
                result = &mut operation => return result,
                changed = async {
                    match self.model_progress.as_mut() {
                        Some((_, receiver)) => receiver.changed().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if changed.is_ok() { self.publish_snapshot(); }
                    else { self.model_progress = None; }
                },
                refusal = self.output_refusal_rx.changed() => {
                    // A producer that could not retain the next chunk reports it here the moment it
                    // happens: a slow tool would otherwise hold the typed fault and the admission
                    // block invisible until it finally returned. The latching is idempotent, so the
                    // release path observing the same refusal again is a no-op.
                    if refusal.is_ok() {
                        let reported = self.output_refusal_rx.borrow_and_update().clone();
                        if let Some(refusal) = reported {
                            self.latch_output_budget_fault(
                                &refusal.operation_id,
                                refusal.accepted,
                                refusal.limit,
                            );
                        }
                    }
                },
                Some(completion) = futures::StreamExt::next(&mut self.background), if !self.background.is_empty() => {
                    self.finish_background(completion);
                },
                message = self.mailbox.recv(), if !self.mailbox.is_closed() || !self.mailbox.is_empty() => {
                    if let Some(message) = message {
                        self.dispatch_mailbox(message).await;
                    }
                }
            }
        }
    }
}

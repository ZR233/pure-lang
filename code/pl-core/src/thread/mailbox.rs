//! Bounded owner commands that cannot modify an admitted model input.
use super::*;

#[derive(Debug)]
pub(super) enum MailboxCommand {
    ToolProgress {
        caller: String,
        executor: crate::tool::opaque::ExecutionAuthority,
        content: Vec<ContextContent>,
        reply: oneshot::Sender<Result<(), ThreadError>>,
    },
    QueueModelUpdate(
        super::model_update::ModelUpdate,
        oneshot::Sender<Result<(), ThreadError>>,
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
}

impl Owner {
    pub(super) fn process_mailbox(&mut self, command: MailboxCommand) {
        match command {
            MailboxCommand::ToolProgress {
                caller,
                executor,
                content,
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
                {
                    Err(ThreadError::TaskAccessExpired)
                } else if !executor.remains_authorized(&self.tools) {
                    Err(ThreadError::ToolPermissionRevoked)
                } else {
                    self.state.tool_progress.insert(caller, content);
                    self.publish_snapshot();
                    Ok(())
                };
                let _ = reply.send(result);
            }
            MailboxCommand::QueueModelUpdate(update, reply) => {
                let result = self.queue_model_update(update);
                let _ = reply.send(result);
            }
            MailboxCommand::BeginIdleClose(reply) => {
                let idle = self.active_input.is_none()
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
                    && self.state.inbox.len() as u64 == self.state.consumed_messages
                    && self.pending.is_empty()
                    && self.uncommitted_tools.is_empty();
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
                    .find(|turn| turn.state == TurnState::Running);
                let result = match active {
                    Some(turn) if expected.as_ref().is_some_and(|id| id != &turn.turn_id) => {
                        Err(ThreadError::InvalidIdentity)
                    }
                    Some(_) => {
                        self.input_driver = None;
                        let interrupted = self.interrupt.interrupt();
                        self.publish_snapshot();
                        Ok(interrupted)
                    }
                    None => Ok(false),
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
                    self.input_driver_error = Some(Arc::new(error));
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
                    self.input_driver_error = Some(Arc::new(error));
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
                Some(completion) = futures::StreamExt::next(&mut self.background), if !self.background.is_empty() => {
                    self.finish_background(completion);
                },
                message = self.mailbox.recv(), if !self.mailbox.is_closed() || !self.mailbox.is_empty() => {
                    if let Some(message) = message {
                        self.process_mailbox(message);
                    }
                }
            }
        }
    }
}

//! Restricted task capabilities; weak commands never keep their Thread alive.
use super::*;

/// A coherent wait result; pending messages remain unconsumed until model admission.
#[derive(Debug, Clone)]
pub struct TaskWaitSnapshot {
    pub tasks: Vec<task::TaskRecord>,
    pub messages_ready: bool,
}

/// Thread-local task reads and explicitly granted controls for one running tool call.
#[derive(Debug, Clone)]
pub struct TaskAccess {
    executor: crate::tool::opaque::ExecutionAuthority,
    authorization: Option<crate::tool::opaque::ToolAuthorization>,
    snapshots: watch::Receiver<ThreadSnapshot>,
    commands: mpsc::WeakSender<mailbox::MailboxCommand>,
    caller: String,
    may_wait: bool,
    may_cancel: bool,
}

impl TaskAccess {
    pub(super) fn new(
        owner: &Owner,
        caller: &str,
        executor: &crate::tool::opaque::FrozenTool,
    ) -> Self {
        let permissions = executor.task_permissions();
        Self {
            executor: executor.authority(),
            authorization: permissions.authorization,
            snapshots: owner.publish.subscribe(),
            commands: owner.task_commands.clone(),
            caller: format!("task:{caller}"),
            may_wait: permissions.wait,
            may_cancel: permissions.cancel,
        }
    }

    pub(crate) async fn validate_execution(&self) -> Result<(), ThreadError> {
        let commands = self.commands.upgrade().ok_or(ThreadError::Closed)?;
        let (reply, response) = oneshot::channel();
        commands
            .send(mailbox::MailboxCommand::ValidateExecution {
                caller: self.caller.clone(),
                executor: self.executor.clone(),
                reply,
            })
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Requests one execution decision for this exact running call and waits without consuming context.
    ///
    /// # Errors
    /// Rejects expired or revoked callers, cancellation, conflicting repeated prompts and closed owners.
    pub async fn request_execution_permission(
        &self,
        payload: OpaquePayload,
        cancellation: CancellationToken,
    ) -> Result<permissions::PermissionDecision, ThreadError> {
        if cancellation.is_cancelled() {
            return Err(ThreadError::Cancelled);
        }
        let commands = self.commands.upgrade().ok_or(ThreadError::Closed)?;
        let (reply, response) = oneshot::channel();
        commands
            .send(mailbox::MailboxCommand::RequestPermission {
                caller: self.caller.clone(),
                authority: self.executor.clone(),
                payload,
                reply,
            })
            .await
            .map_err(|_| ThreadError::Closed)?;
        let record = response.await.map_err(|_| ThreadError::Closed)??;
        let mut snapshots = self.snapshots.clone();
        loop {
            if cancellation.is_cancelled() {
                return Err(ThreadError::Cancelled);
            }
            {
                let snapshot = snapshots.borrow_and_update();
                self.ensure_running(&snapshot)?;
                let current = snapshot
                    .permissions
                    .get(&record.id)
                    .ok_or(ThreadError::InvalidIdentity)?;
                match current.state {
                    permissions::PermissionState::Allowed => {
                        return Ok(permissions::PermissionDecision::Allow);
                    }
                    permissions::PermissionState::Denied => {
                        return Ok(permissions::PermissionDecision::Deny);
                    }
                    permissions::PermissionState::Cancelled => return Err(ThreadError::Cancelled),
                    permissions::PermissionState::Pending => {}
                }
            }
            tokio::select! {
                _ = cancellation.cancelled() => return Err(ThreadError::Cancelled),
                changed = snapshots.changed() => changed.map_err(|_| ThreadError::Closed)?,
            }
        }
    }

    /// Replaces this running call's transient preview without committing history or model context.
    /// The producer bounds and coalesces updates; at most 64 KiB of portable content is accepted.
    ///
    /// # Errors
    /// Rejects oversized previews, expired or revoked executors and a closed owner.
    pub async fn report_progress(&self, content: Vec<ContextContent>) -> Result<(), ThreadError> {
        if content.len() > 256 || progress_bytes(&content) > 64 * 1024 {
            return Err(ThreadError::InvalidOutput);
        }
        self.ensure_running(&self.snapshots.borrow())?;
        let commands = self.commands.upgrade().ok_or(ThreadError::Closed)?;
        let (reply, response) = oneshot::channel();
        commands
            .send(mailbox::MailboxCommand::ToolProgress {
                caller: self.caller.clone(),
                executor: self.executor.clone(),
                content,
                reply,
            })
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Lists this Thread's immutable task metadata in stable task-identity order.
    /// Reading does not consume messages, authorize cancellation or keep the owner alive.
    pub fn list(&self) -> Vec<task::TaskRecord> {
        self.snapshots.borrow().tasks.values().cloned().collect()
    }

    /// Reads immutable task metadata without consuming a message or result.
    ///
    /// # Errors
    /// Rejects a task not owned by this Thread.
    pub fn get(&self, id: &str) -> Result<task::TaskRecord, ThreadError> {
        self.snapshots
            .borrow()
            .tasks
            .get(id)
            .cloned()
            .ok_or(ThreadError::InvalidIdentity)
    }

    /// Reads a terminal result; `None` means execution has not committed a terminal state.
    ///
    /// # Errors
    /// Rejects unknown tasks or an inconsistent terminal result reference.
    pub fn result(&self, id: &str) -> Result<Option<ToolDelivery>, ThreadError> {
        let snapshot = self.snapshots.borrow();
        let task = snapshot.tasks.get(id).ok_or(ThreadError::InvalidIdentity)?;
        if snapshot.pending_tool_commits.contains(&task.call_id) {
            return Err(ThreadError::PendingToolCommit);
        }
        if task.status == task::TaskStatus::Running {
            return Ok(None);
        }
        snapshot
            .deliveries
            .iter()
            .find(|delivery| delivery.call_id == task.call_id)
            .cloned()
            .map(Some)
            .ok_or(ThreadError::InvalidOutput)
    }

    /// Waits for any selected task to finish or for a pending Thread message.
    /// Empty IDs wait only for messages. This operation never consumes inbox contents.
    ///
    /// # Errors
    /// Rejects missing permission, self-waits, stale callers, unknown tasks and cancelled waits.
    pub async fn wait(
        &self,
        ids: &[String],
        cancellation: CancellationToken,
    ) -> Result<TaskWaitSnapshot, ThreadError> {
        if !self.may_wait {
            return Err(ThreadError::TaskAccessDenied);
        }
        if ids.iter().any(|id| id == &self.caller) {
            return Err(ThreadError::TaskSelfWait);
        }
        let mut snapshots = self.snapshots.clone();
        loop {
            if cancellation.is_cancelled() {
                return Err(ThreadError::Cancelled);
            }
            {
                let snapshot = snapshots.borrow_and_update();
                self.ensure_running(&snapshot)?;
                let tasks = ids
                    .iter()
                    .map(|id| {
                        snapshot
                            .tasks
                            .get(id)
                            .cloned()
                            .ok_or(ThreadError::InvalidIdentity)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if tasks
                    .iter()
                    .any(|task| snapshot.pending_tool_commits.contains(&task.call_id))
                {
                    return Err(ThreadError::PendingToolCommit);
                }
                let messages_ready = snapshot.inbox.len() as u64 > snapshot.consumed_messages;
                if messages_ready
                    || tasks
                        .iter()
                        .any(|task| task.status != task::TaskStatus::Running)
                {
                    return Ok(TaskWaitSnapshot {
                        tasks,
                        messages_ready,
                    });
                }
            }
            tokio::select! {
                _ = cancellation.cancelled() => return Err(ThreadError::Cancelled),
                changed = snapshots.changed() => changed.map_err(|_| ThreadError::Closed)?,
            }
        }
    }

    /// Requests cancellation only while the granted caller task is still active.
    ///
    /// # Errors
    /// Rejects missing permission, expired callers, unknown targets or a closed owner.
    pub async fn cancel(&self, id: String) -> Result<task::TaskCancellationReceipt, ThreadError> {
        if !self.may_cancel {
            return Err(ThreadError::TaskAccessDenied);
        }
        self.ensure_running(&self.snapshots.borrow())?;
        let commands = self.commands.upgrade().ok_or(ThreadError::Closed)?;
        let (reply, response) = oneshot::channel();
        commands
            .send(mailbox::MailboxCommand::ToolCancelTask {
                caller: self.caller.clone(),
                authorization: self.authorization.clone(),
                target: id,
                reply,
            })
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    fn ensure_running(&self, snapshot: &ThreadSnapshot) -> Result<(), ThreadError> {
        if snapshot
            .tasks
            .get(&self.caller)
            .is_some_and(|task| task.status == task::TaskStatus::Running)
        {
            Ok(())
        } else {
            Err(ThreadError::TaskAccessExpired)
        }
    }
}

fn progress_bytes(content: &[ContextContent]) -> usize {
    content.iter().fold(0usize, |bytes, item| {
        bytes.saturating_add(match item {
            ContextContent::Text { text } => text.len(),
            ContextContent::Opaque { payload } => payload
                .content()
                .len()
                .saturating_add(payload.format().len()),
            ContextContent::Resource { reference } => reference
                .id()
                .len()
                .saturating_add(reference.content_digest().len())
                .saturating_add(reference.media_type().len()),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{ModelSession, ModelToolCall, PreparedModelCall},
        tool::{
            ToolOutput,
            opaque::{CallContext, Registration, Tool, ToolError},
        },
    };
    use pretty_assertions::assert_eq;

    struct ModelCall;
    impl ModelSession for ModelCall {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: Vec::new(),
                    tool_calls: vec![ModelToolCall {
                        call_id: "call".into(),
                        tool_id: "tool".into(),
                        arguments: OpaquePayload::text("input"),
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

    #[derive(Debug)]
    struct ProgressTool {
        access: mpsc::UnboundedSender<TaskAccess>,
        release: Arc<tokio::sync::Notify>,
    }
    impl Tool for ProgressTool {
        async fn execute(
            &self,
            _: OpaquePayload,
            context: CallContext,
        ) -> Result<ToolOutput, ToolError> {
            self.access.send(context.tasks.unwrap()).unwrap();
            self.release.notified().await;
            Ok(ToolOutput::new(
                OpaquePayload::text("complete"),
                vec![ContextContent::Text {
                    text: "final output".into(),
                }],
            ))
        }
    }

    #[tokio::test]
    async fn task_progress_is_live_only_and_expires_at_completion_or_executor_revocation() {
        for revoked in [false, true] {
            let thread =
                ThreadHandle::start("progress".into(), DynModelSession::new(ModelCall)).unwrap();
            let (sender, mut receiver) = mpsc::unbounded_channel();
            let release = Arc::new(tokio::sync::Notify::new());
            thread
                .register_tools(vec![
                    Registration::new(
                        "tool".into(),
                        OpaquePayload::text("declaration"),
                        ProgressTool {
                            access: sender.clone(),
                            release: release.clone(),
                        },
                    )
                    .unwrap()
                    .foreground(),
                ])
                .await
                .unwrap();
            thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: Vec::new(),
                    cancellation: CancellationToken::new(),
                })
                .await
                .unwrap();
            let execution = tokio::spawn({
                let thread = thread.clone();
                async move {
                    thread
                        .execute_tool("call".into(), CancellationToken::new())
                        .await
                }
            });
            let access = receiver.recv().await.unwrap();
            let before = thread.snapshot();
            let preview = vec![ContextContent::Text {
                text: "stdout before exit".into(),
            }];
            access.report_progress(preview.clone()).await.unwrap();
            let live = thread.snapshot();
            assert_eq!(live.tool_progress.get("task:call"), Some(&preview));
            assert_eq!(live.commit_sequence, before.commit_sequence);
            assert_eq!(live.context, before.context);
            assert!(
                serde_json::to_value(&live)
                    .unwrap()
                    .get("toolProgress")
                    .is_none()
            );
            assert!(matches!(
                access
                    .report_progress(vec![ContextContent::Text {
                        text: "x".repeat(65537).into()
                    }])
                    .await,
                Err(ThreadError::InvalidOutput)
            ));
            if revoked {
                thread.register_tools(Vec::new()).await.unwrap();
                assert!(matches!(
                    access.report_progress(preview.clone()).await,
                    Err(ThreadError::ToolPermissionRevoked)
                ));
            }
            release.notify_one();
            execution.await.unwrap().unwrap();
            let completed = thread.snapshot();
            assert!(completed.tool_progress.is_empty());
            assert_eq!(
                completed.deliveries[0].delivered_context,
                vec![ContextContent::Text {
                    text: "final output".into()
                }]
            );
            assert!(matches!(
                access.report_progress(preview).await,
                Err(ThreadError::TaskAccessExpired)
            ));
            let replay = journal::replay(&thread.journal().await.unwrap()).unwrap();
            assert!(replay.tool_progress.is_empty());
            thread.close().await.unwrap();
        }
    }
}

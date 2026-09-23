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
    effect_window: Arc<EffectWindow>,
    thread_id: String,
    /// Durable store handle, if this Thread is persisted. Reads only; it never keeps the owner alive.
    cold: Option<cold::ColdStoreHandle>,
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
            effect_window: owner.effect_window.clone(),
            thread_id: owner.id.clone(),
            cold: owner.cold.clone(),
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
        let snapshot = self.snapshots.borrow();
        snapshot
            .tasks
            .get(id)
            .or_else(|| {
                snapshot
                    .terminal_tasks
                    .iter()
                    .find(|record| record.id == id)
            })
            .cloned()
            .ok_or_else(|| ThreadError::TaskNotFound { task_id: id.into() })
    }

    /// Reads a terminal result; `None` means execution has not committed a terminal state.
    ///
    /// # Errors
    /// Rejects unknown tasks or an inconsistent terminal result reference.
    pub fn result(&self, id: &str) -> Result<Option<ToolDelivery>, ThreadError> {
        let snapshot = self.snapshots.borrow();
        let task = snapshot
            .tasks
            .get(id)
            .or_else(|| {
                snapshot
                    .terminal_tasks
                    .iter()
                    .find(|record| record.id == id)
            })
            .ok_or_else(|| ThreadError::TaskNotFound { task_id: id.into() })?;
        if snapshot.pending_tool_commits.contains(&task.call_id) {
            return Err(ThreadError::PendingToolCommit);
        }
        if task.status == task::TaskStatus::Running {
            return Ok(None);
        }
        // A settled result is committed history. The resident queue answers a result still owed to
        // model context; otherwise the exact committed delivery is read back from the bounded live
        // effect window. Older results are answered by the host's calls reader (by
        // `(thread_id, call_id)`), never by a checkpoint-visible payload ledger.
        let call_id = task.call_id.as_str();
        if let Some(delivery) = snapshot
            .deliveries
            .iter()
            .find(|delivery| delivery.call_id == call_id)
        {
            return Ok(Some(delivery.clone()));
        }
        match recent_effect_fact(&self.effect_window, |effect| {
            effect
                .deliveries
                .iter()
                .rev()
                .find(|delivery| delivery.call_id == call_id)
                .cloned()
        }) {
            Some(delivery) => Ok(Some(delivery)),
            None => Err(ThreadError::InvalidOutput),
        }
    }

    /// Reads a task's durable fact when the owner no longer retains its transient identity/result.
    ///
    /// The resident task ledgers and the live effect window are both bounded, so a finished task
    /// that left the window must be answered by the host's durable call facts instead of being
    /// reported as unknown or revived. This is a read-only lookup: it grants no permission, starts no
    /// execution and does not keep the owner alive. `Ok(None)` means the durable store does not know
    /// the identity either, and a non-terminal record must not be presented as a finished result.
    ///
    /// # Errors
    /// Surfaces storage failures; it never fabricates a terminal state.
    pub async fn durable(&self, id: &str) -> Result<Option<cold::DurableToolTask>, ThreadError> {
        let Some(cold) = &self.cold else {
            return Ok(None);
        };
        cold.read_tool_task(&self.thread_id, id)
            .await
            .map_err(|error| ThreadError::Storage(Arc::new(error)))
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
                            .or_else(|| {
                                snapshot
                                    .terminal_tasks
                                    .iter()
                                    .find(|record| record.id == *id)
                            })
                            .cloned()
                            .ok_or_else(|| ThreadError::TaskNotFound { task_id: id.into() })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if tasks
                    .iter()
                    .any(|task| snapshot.pending_tool_commits.contains(&task.call_id))
                {
                    return Err(ThreadError::PendingToolCommit);
                }
                // The resident queue holds only pending messages, so readiness is a sequence
                // comparison against the consumption watermark, never a queue length.
                let messages_ready = snapshot
                    .inbox
                    .iter()
                    .any(|record| record.sequence > snapshot.consumed_messages);
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
                target: id.clone(),
                reply,
            })
            .await
            .map_err(|_| ThreadError::Closed)?;
        match response.await.map_err(|_| ThreadError::Closed)? {
            Ok(receipt) => Ok(receipt),
            // The owner's terminal task ledger is bounded, so a finished task that left it is
            // adjudicated from the durable task lifecycle instead of being reported as unknown.
            // A durable terminal row is already finished, and a recorded cancellation request is
            // already requested: neither answer revives the task nor starts new work.
            Err(ThreadError::TaskNotFound { .. }) => {
                match self.durable(&id).await? {
                    Some(fact) if fact.task.status != task::TaskStatus::Running => {
                        Ok(task::TaskCancellationReceipt::AlreadyFinished)
                    }
                    Some(fact) if fact.task.cancel_requested => {
                        Ok(task::TaskCancellationReceipt::AlreadyRequested)
                    }
                    // A still-running task the owner does not know about is a real inconsistency:
                    // report it instead of inventing a cancellation receipt.
                    _ => Err(ThreadError::TaskNotFound { task_id: id }),
                }
            }
            Err(error) => Err(error),
        }
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

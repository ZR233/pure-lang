//! Task identity and lifecycle facts, independent of tool business payloads.
use super::*;

/// A scheduler state; failure details and complete output remain on the linked delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl TaskStatus {
    pub(super) fn from_outcome(outcome: &ToolOutcome) -> Self {
        match outcome {
            ToolOutcome::Succeeded => Self::Succeeded,
            ToolOutcome::Failed(_) => Self::Failed,
            ToolOutcome::Cancelled => Self::Cancelled,
            ToolOutcome::Interrupted => Self::Interrupted,
        }
    }
}

/// A cancellation acknowledgement, separate from the task's eventual terminal status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskCancellationReceipt {
    Requested,
    AlreadyRequested,
    AlreadyFinished,
}

/// Identifies the exact context version containing the accepted task receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAcknowledgement {
    pub record_id: String,
    pub context_revision: u64,
}

/// An immutable task revision, linked to the original call and its eventual delivery.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub call_id: String,
    pub tool_id: String,
    pub turn_id: String,
    pub revision: u64,
    pub status: TaskStatus,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default)]
    pub acknowledgement: Option<TaskAcknowledgement>,
}

impl Owner {
    pub(super) fn start_task(&mut self, call: &PendingCall) -> Result<(), ThreadError> {
        let record = TaskRecord {
            id: format!("task:{}", call.call.call_id),
            call_id: call.call.call_id.clone(),
            tool_id: call.call.tool_id.clone(),
            turn_id: call.turn_id.clone(),
            revision: 1,
            status: TaskStatus::Running,
            cancel_requested: false,
            acknowledgement: None,
        };
        if self.state.tasks.contains_key(&record.id) {
            return Err(ThreadError::InvalidIdentity);
        }
        record_change(&mut self.state, record);
        self.publish();
        Ok(())
    }

    pub(super) fn cancel_task(&mut self, id: &str) -> Result<TaskCancellationReceipt, ThreadError> {
        let mut record = self
            .state
            .tasks
            .get(id)
            .cloned()
            .ok_or_else(|| ThreadError::TaskNotFound { task_id: id.into() })?;
        if record.status != TaskStatus::Running {
            return Ok(TaskCancellationReceipt::AlreadyFinished);
        }
        if record.cancel_requested {
            return Ok(TaskCancellationReceipt::AlreadyRequested);
        }
        let token = self
            .task_tokens
            .get(id)
            .cloned()
            .ok_or(ThreadError::InvalidIdentity)?;
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        record.cancel_requested = true;
        record_change(&mut self.state, record);
        self.publish();
        token.cancel();
        Ok(TaskCancellationReceipt::Requested)
    }

    pub(super) fn finish_task(
        &mut self,
        call_id: &str,
        outcome: &ToolOutcome,
    ) -> Result<(), ThreadError> {
        let mut record = self
            .state
            .tasks
            .get(&format!("task:{call_id}"))
            .cloned()
            .ok_or(ThreadError::InvalidIdentity)?;
        if record.status != TaskStatus::Running {
            return Err(ThreadError::InvalidIdentity);
        }
        record.status = TaskStatus::from_outcome(outcome);
        record.cancel_requested |= record.status == TaskStatus::Cancelled;
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        record_change(&mut self.state, record);
        Ok(())
    }
}

pub(super) fn record_change(state: &mut ThreadSnapshot, record: TaskRecord) {
    state.tasks.insert(record.id.clone(), record.clone());
    let mut changes = state.task_changes.to_vec();
    changes.push(record);
    state.task_changes = changes.into();
}

pub(super) fn replay(
    state: &mut ThreadSnapshot,
    commit: &journal::ThreadCommit,
) -> Result<(), ThreadError> {
    for record in commit.tasks.iter() {
        if record.id != format!("task:{}", record.call_id)
            || record.call_id.is_empty()
            || record.turn_id.is_empty()
        {
            return Err(ThreadError::InvalidIdentity);
        }
        if let Some(previous) = state.tasks.get(&record.id) {
            if previous.status != TaskStatus::Running
                || previous.call_id != record.call_id
                || previous.tool_id != record.tool_id
                || previous.turn_id != record.turn_id
                || previous.revision.checked_add(1) != Some(record.revision)
            {
                return Err(ThreadError::InvalidOutput);
            }
            if record.status == TaskStatus::Running {
                let cancelled = !previous.cancel_requested
                    && record.cancel_requested
                    && previous.acknowledgement == record.acknowledgement;
                let acknowledged = previous.acknowledgement.is_none()
                    && record.acknowledgement.is_some()
                    && previous.cancel_requested == record.cancel_requested;
                if (!cancelled && !acknowledged)
                    || commit
                        .deliveries
                        .iter()
                        .any(|delivery| delivery.call_id == record.call_id)
                {
                    return Err(ThreadError::InvalidOutput);
                }
                if acknowledged {
                    let receipt = record
                        .acknowledgement
                        .as_ref()
                        .ok_or(ThreadError::InvalidOutput)?;
                    let Some(journal::ContextChange::Append { revision, records }) =
                        &commit.context
                    else {
                        return Err(ThreadError::InvalidContext);
                    };
                    if *revision != receipt.context_revision || !records.iter().any(|context| {
                        context.id == receipt.record_id && context.turn_id.as_deref() == Some(record.turn_id.as_str())
                            && matches!(&context.source, ContextSource::ToolResult { call_id, tool_id } if call_id == &record.call_id && tool_id == &record.tool_id)
                    }) { return Err(ThreadError::InvalidContext); }
                }
            } else {
                let expected_cancel =
                    previous.cancel_requested || record.status == TaskStatus::Cancelled;
                if record.acknowledgement != previous.acknowledgement
                    || record.cancel_requested != expected_cancel
                    || (previous.cancel_requested
                        && !matches!(
                            record.status,
                            TaskStatus::Cancelled | TaskStatus::Interrupted
                        ))
                {
                    return Err(ThreadError::InvalidOutput);
                }
                let delivery = commit
                    .deliveries
                    .iter()
                    .find(|delivery| delivery.call_id == record.call_id)
                    .ok_or(ThreadError::InvalidOutput)?;
                if matches!(delivery.target, ToolDeliveryTarget::Inbox { .. })
                    != record.acknowledgement.is_some()
                    || delivery.tool_id != record.tool_id
                    || TaskStatus::from_outcome(&delivery.outcome) != record.status
                {
                    return Err(ThreadError::InvalidOutput);
                }
            }
        } else {
            if record.status != TaskStatus::Running
                || record.revision != 1
                || record.cancel_requested
                || record.acknowledgement.is_some()
                || state.lifecycle != ThreadLifecycle::Open
            {
                return Err(ThreadError::InvalidOutput);
            }
            let pending = state.context.pending_calls()?;
            if !pending
                .iter()
                .any(|call| call.call_id == record.call_id && call.tool_id == record.tool_id)
                || !state.context.records.iter().any(|context| {
                    context.turn_id.as_deref() == Some(&record.turn_id)
                        && context
                            .tool_calls
                            .iter()
                            .any(|call| call.call_id == record.call_id)
                })
            {
                return Err(ThreadError::InvalidContext);
            }
        }
        record_change(state, record.clone());
    }
    Ok(())
}

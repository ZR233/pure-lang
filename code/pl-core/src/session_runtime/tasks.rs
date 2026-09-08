use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{ToolTaskPage, ToolTaskReceipt, ToolTaskResult, ToolTaskSnapshot, ToolTaskStatus};

const MAX_TASKS: usize = 4096;
const MAX_ACTIVE_TASKS: usize = 64;
const MAX_RETAINED_RESULT_BYTES: usize = 64 * 1024 * 1024;

/// Validation failures from the session owner's task state machine.
#[derive(Debug, thiserror::Error)]
pub enum SessionTaskError {
    #[error("session tool task capacity reached")]
    Capacity,
    #[error("invalid task identity: {field}")]
    InvalidIdentity { field: &'static str },
    #[error("task {id} is not owned by this session")]
    NotFound { id: String },
    #[error("task {id} conflicts with its accepted invocation")]
    IdentityConflict { id: String },
    #[error("invalid transition for task {id}: {from:?} -> {to:?}")]
    InvalidTransition {
        id: String,
        from: ToolTaskStatus,
        to: ToolTaskStatus,
    },
    #[error("invalid task snapshot: {reason}")]
    InvalidSnapshot { reason: &'static str },
    #[error("cannot encode tool task result")]
    Encoding(#[from] serde_json::Error),
}

/// Durable task facts, without executable closures, locks, or process handles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionTasksSnapshot {
    entries: BTreeMap<String, TaskRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskRecord {
    pub(crate) snapshot: ToolTaskSnapshot,
    arguments_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) complete_result: Option<super::result_record::TaskResultRecord>,
}

/// Task facts owned by the Thread actor, independently of any model Turn.
#[derive(Debug, Clone, Default)]
pub struct SessionTasks {
    state: Arc<SessionTasksSnapshot>,
}

impl SessionTasks {
    pub(crate) fn storage_records(&self) -> impl Iterator<Item = (&String, &TaskRecord)> {
        self.state.entries.iter()
    }

    pub(crate) fn restore_records(
        entries: BTreeMap<String, TaskRecord>,
    ) -> Result<Self, SessionTaskError> {
        Self::restore(SessionTasksSnapshot { entries })
    }
    pub(crate) fn complete_result(
        &self,
        id: &str,
    ) -> Result<Option<&super::result_record::TaskResultRecord>, SessionTaskError> {
        self.state
            .entries
            .get(id)
            .map(|entry| entry.complete_result.as_ref())
            .ok_or_else(|| SessionTaskError::NotFound { id: id.to_owned() })
    }

    pub(crate) fn release_saved_results(&mut self) {
        if self.state.entries.values().all(|entry| {
            entry
                .complete_result
                .as_ref()
                .is_none_or(|result| result.content.is_none())
        }) {
            return;
        }
        for entry in Arc::make_mut(&mut self.state).entries.values_mut() {
            if let Some(result) = &mut entry.complete_result {
                result.content = None;
            }
        }
    }

    pub(crate) fn records(&self) -> impl Iterator<Item = &ToolTaskSnapshot> {
        self.state.entries.values().map(|record| &record.snapshot)
    }

    pub(crate) fn set_delivery(
        &mut self,
        id: &str,
        delivery: super::ToolTaskDelivery,
    ) -> Result<(), SessionTaskError> {
        let record = Arc::make_mut(&mut self.state)
            .entries
            .get_mut(id)
            .ok_or_else(|| SessionTaskError::NotFound { id: id.to_owned() })?;
        record.snapshot.delivery = delivery;
        Ok(())
    }
    /// Returns immutable state for an owner checkpoint.
    pub fn snapshot(&self) -> &SessionTasksSnapshot {
        &self.state
    }

    pub(crate) fn execution_changes_since<'a>(
        &'a self,
        previous: &'a Self,
    ) -> impl Iterator<Item = &'a ToolTaskSnapshot> {
        let count = if Arc::ptr_eq(&self.state, &previous.state) {
            0
        } else {
            usize::MAX
        };
        self.state
            .entries
            .iter()
            .take(count)
            .filter_map(move |(id, record)| {
                previous
                    .state
                    .entries
                    .get(id)
                    .is_none_or(|old| {
                        let old = &old.snapshot;
                        let next = &record.snapshot;
                        old.receipt != next.receipt
                            || old.status != next.status
                            || old.created_at != next.created_at
                            || old.updated_at != next.updated_at
                            || old.result != next.result
                    })
                    .then_some(&record.snapshot)
            })
    }

    /// Restores task facts without restarting physical work.
    ///
    /// # Errors
    /// Rejects invalid identities, mismatched terminal output, and capacity violations.
    pub fn restore(state: SessionTasksSnapshot) -> Result<Self, SessionTaskError> {
        if state.entries.len() > MAX_TASKS {
            return Err(SessionTaskError::Capacity);
        }
        for (id, entry) in &state.entries {
            validate_receipt(&entry.snapshot.receipt)?;
            if id != &entry.snapshot.receipt.task_id
                || !valid_hash(&entry.arguments_hash)
                || entry.snapshot.status.is_terminal() != entry.snapshot.result.is_some()
            {
                return Err(SessionTaskError::InvalidSnapshot {
                    reason: "invalid task record",
                });
            }
            match &entry.complete_result {
                Some(result) => {
                    result.validate(&entry.snapshot)?;
                }
                None if entry.snapshot.status.is_terminal() => {
                    return Err(SessionTaskError::InvalidSnapshot {
                        reason: "terminal task has no complete result identity",
                    });
                }
                None => {}
            }
        }
        let tasks = Self {
            state: Arc::new(state),
        };
        if tasks.active_ids().count() > MAX_ACTIVE_TASKS {
            return Err(SessionTaskError::Capacity);
        }
        Ok(tasks)
    }

    /// Stages acceptance, returning false for an exact already accepted invocation.
    ///
    /// # Errors
    /// Rejects conflicting invocation identities, malformed values, or full task capacity.
    pub fn admit(
        &mut self,
        receipt: ToolTaskReceipt,
        arguments_hash: String,
        created_at: i64,
    ) -> Result<bool, SessionTaskError> {
        validate_receipt(&receipt)?;
        if !valid_hash(&arguments_hash) {
            return Err(SessionTaskError::InvalidIdentity {
                field: "argumentsHash",
            });
        }
        if let Some(existing) = self.state.entries.get(&receipt.task_id) {
            return if existing.snapshot.receipt.thread_id == receipt.thread_id
                && existing.snapshot.receipt.turn_id == receipt.turn_id
                && existing.snapshot.receipt.call_id == receipt.call_id
                && existing.snapshot.receipt.tool_name == receipt.tool_name
                && existing.arguments_hash == arguments_hash
            {
                Ok(false)
            } else {
                Err(SessionTaskError::IdentityConflict {
                    id: receipt.task_id,
                })
            };
        }
        if self.state.entries.len() >= MAX_TASKS
            || self.active_ids().count() >= MAX_ACTIVE_TASKS
            || self
                .state
                .entries
                .values()
                .filter_map(|entry| entry.complete_result.as_ref())
                .filter(|result| result.content.is_some())
                .fold(0u64, |bytes, result| {
                    bytes.saturating_add(result.identity.encoded_bytes)
                })
                >= MAX_RETAINED_RESULT_BYTES as u64
        {
            return Err(SessionTaskError::Capacity);
        }
        let id = receipt.task_id.clone();
        Arc::make_mut(&mut self.state).entries.insert(
            id,
            TaskRecord {
                snapshot: ToolTaskSnapshot {
                    receipt,
                    status: ToolTaskStatus::Queued,
                    delivery: super::ToolTaskDelivery::PendingResponse,
                    created_at,
                    updated_at: created_at,
                    result: None,
                    result_reference: None,
                },
                arguments_hash,
                complete_result: None,
            },
        );
        Ok(true)
    }

    /// Reads a task without consuming its terminal event.
    ///
    /// # Errors
    /// Returns NotFound for a handle outside this session or its retained task history.
    pub fn get(&self, id: &str) -> Result<&ToolTaskSnapshot, SessionTaskError> {
        self.state
            .entries
            .get(id)
            .map(|entry| &entry.snapshot)
            .ok_or_else(|| SessionTaskError::NotFound { id: id.to_owned() })
    }

    /// Lists tasks in stable task-ID order. No status filter selects nonterminal tasks.
    ///
    /// The cursor is an exclusive task ID; querying does not consume wake events.
    pub fn list(&self, status: Option<ToolTaskStatus>, cursor: Option<&str>) -> ToolTaskPage {
        let lower = cursor.map_or(Bound::Unbounded, Bound::Excluded);
        let mut matches = self
            .state
            .entries
            .range::<str, _>((lower, Bound::Unbounded))
            .map(|(_, record)| &record.snapshot)
            .filter(|task| {
                status.map_or(!task.status.is_terminal(), |status| task.status == status)
            });
        let tasks: Vec<_> = matches
            .by_ref()
            .take(64)
            .map(super::ToolTaskSummary::from)
            .collect();
        let next_cursor = matches
            .next()
            .and_then(|_| tasks.last().map(|task| task.receipt.task_id.clone()));
        ToolTaskPage { tasks, next_cursor }
    }

    /// Iterates live task IDs for residency, cancellation, and crash recovery.
    pub fn active_ids(&self) -> impl Iterator<Item = &str> {
        self.state
            .entries
            .iter()
            .filter(|(_, record)| !record.snapshot.status.is_terminal())
            .map(|(id, _)| id.as_str())
    }

    /// Stages a nonterminal execution phase or cancellation request.
    ///
    /// # Errors
    /// Rejects missing tasks and transitions inconsistent with the task lifecycle.
    pub fn transition(
        &mut self,
        id: &str,
        to: ToolTaskStatus,
        now: i64,
    ) -> Result<(), SessionTaskError> {
        use ToolTaskStatus::{Cancelling, Queued, Running, WaitingApproval};
        let from = self.get(id)?.status;
        if from == to && !from.is_terminal() {
            return Ok(());
        }
        if !matches!(
            (from, to),
            (Queued, WaitingApproval | Running | Cancelling)
                | (WaitingApproval, Running | Cancelling)
                | (Running, Cancelling)
        ) {
            return Err(SessionTaskError::InvalidTransition {
                id: id.to_owned(),
                from,
                to,
            });
        }
        let entry = Arc::make_mut(&mut self.state)
            .entries
            .get_mut(id)
            .ok_or_else(|| SessionTaskError::NotFound { id: id.to_owned() })?;
        entry.snapshot.status = to;
        entry.snapshot.updated_at = now;
        Ok(())
    }

    /// Stages a terminal result after physical execution has exited.
    ///
    /// # Errors
    /// Rejects missing tasks, duplicate/conflicting finalization, or invalid states. Complete output is retained independently of its preview.
    pub fn finish(
        &mut self,
        id: &str,
        status: ToolTaskStatus,
        result: ToolTaskResult,
        now: i64,
    ) -> Result<ToolTaskSnapshot, SessionTaskError> {
        let from = self.get(id)?.status;
        if from.is_terminal()
            || !status.is_terminal()
            || status == ToolTaskStatus::Succeeded && from != ToolTaskStatus::Running
            || from == ToolTaskStatus::Cancelling
                && status != ToolTaskStatus::Cancelled
                && status != ToolTaskStatus::Interrupted
        {
            return Err(SessionTaskError::InvalidTransition {
                id: id.to_owned(),
                from,
                to: status,
            });
        }
        let entry = Arc::make_mut(&mut self.state)
            .entries
            .get_mut(id)
            .ok_or_else(|| SessionTaskError::NotFound { id: id.to_owned() })?;
        entry.snapshot.status = status;
        entry.snapshot.updated_at = now;
        let complete =
            super::result_record::TaskResultRecord::new(&entry.snapshot.receipt.task_id, result)?;
        entry.snapshot.result = complete.content.as_deref().cloned();
        entry.snapshot = super::model_view::task(&entry.snapshot).map_err(|_| {
            SessionTaskError::InvalidSnapshot {
                reason: "result preview encoding failed",
            }
        })?;
        entry.complete_result = Some(complete);
        Ok(entry.snapshot.clone())
    }
}

fn validate_receipt(receipt: &ToolTaskReceipt) -> Result<(), SessionTaskError> {
    for (field, value) in [
        ("taskId", &receipt.task_id),
        ("threadId", &receipt.thread_id),
        ("turnId", &receipt.turn_id),
        ("callId", &receipt.call_id),
        ("itemId", &receipt.item_id),
        ("toolName", &receipt.tool_name),
    ] {
        if value.trim().is_empty() || value.len() > 256 {
            return Err(SessionTaskError::InvalidIdentity { field });
        }
    }
    Ok(())
}

fn valid_hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

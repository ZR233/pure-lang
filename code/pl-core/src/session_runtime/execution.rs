use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use futures::{FutureExt, future::BoxFuture};
use tokio::task::{AbortHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::tool::{ToolResult, ToolRuntimeLockPolicy};

use super::{ToolTaskReceipt, ToolTaskStatus};

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolCheckpointChanges {
    pub(crate) direct_results: Vec<String>,
    pub(crate) consumed_events: Option<super::SessionEventBatch>,
}

type TaskExecution =
    dyn FnOnce(CancellationToken) -> BoxFuture<'static, crate::Result<ToolResult>> + Send;

/// Frozen execution prepared by dispatch, never executed before the owner checkpoint.
pub(crate) struct SessionTaskSubmission {
    pub(crate) receipt: ToolTaskReceipt,
    pub(crate) arguments_hash: String,
    pub(crate) lock_policy: ToolRuntimeLockPolicy,
    pub(crate) start_phase: TaskStartPhase,
    pub(crate) execute: Box<TaskExecution>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum TaskStartPhase {
    Approval,
    Execution,
}

impl TaskStartPhase {
    pub(crate) fn status(self) -> ToolTaskStatus {
        match self {
            Self::Approval => ToolTaskStatus::WaitingApproval,
            Self::Execution => ToolTaskStatus::Running,
        }
    }
}

impl fmt::Debug for SessionTaskSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionTaskSubmission")
            .field("receipt", &self.receipt)
            .field("lock_policy", &self.lock_policy)
            .field("start_phase", &self.start_phase)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SessionTaskCompletion {
    pub(crate) id: String,
    pub(crate) output: ToolResult,
    pub(crate) cancelled: bool,
}

struct RunningTask {
    cancellation: CancellationToken,
    abort: AbortHandle,
    lock_policy: ToolRuntimeLockPolicy,
}

/// Physical execution resources. The owning actor alone schedules and releases them.
#[derive(Default)]
pub(crate) struct SessionTaskResources {
    completed_at: BTreeMap<String, tokio::time::Instant>,
    settlement_blocked: bool,
    queued: VecDeque<SessionTaskSubmission>,
    running: BTreeMap<String, RunningTask>,
    workers: JoinSet<SessionTaskCompletion>,
    completed: VecDeque<SessionTaskCompletion>,
}

impl SessionTaskResources {
    pub(crate) fn completed_before(&self, id: &str, deadline: tokio::time::Instant) -> bool {
        self.completed_at.get(id).is_some_and(|at| *at <= deadline)
    }
    pub(crate) fn enqueue(&mut self, submission: SessionTaskSubmission) {
        self.queued.push_back(submission);
    }

    /// Borrows the next schedulable task; launch only after committing its phase.
    pub(crate) fn next_ready(&self) -> Option<&SessionTaskSubmission> {
        self.queued.get(self.next_ready_position()?)
    }

    fn next_ready_position(&self) -> Option<usize> {
        let next = self.queued.front()?;
        let allowed = match next.lock_policy {
            ToolRuntimeLockPolicy::None => true,
            ToolRuntimeLockPolicy::Shared => self
                .running
                .values()
                .all(|task| task.lock_policy != ToolRuntimeLockPolicy::Exclusive),
            ToolRuntimeLockPolicy::Exclusive => self
                .running
                .values()
                .all(|task| task.lock_policy == ToolRuntimeLockPolicy::None),
        };
        if allowed {
            Some(0)
        } else {
            self.queued
                .iter()
                .position(|task| task.lock_policy == ToolRuntimeLockPolicy::None)
        }
    }

    pub(crate) fn launch_next(&mut self) {
        let Some(position) = self.next_ready_position() else {
            return;
        };
        let Some(submission) = self.queued.remove(position) else {
            return;
        };
        let id = submission.receipt.task_id.clone();
        let cancellation = CancellationToken::new();
        let token = cancellation.clone();
        let task_id = id.clone();
        let abort = self.workers.spawn(async move {
            let execution = async { (submission.execute)(token.clone()).await };
            let output = match std::panic::AssertUnwindSafe(execution).catch_unwind().await {
                Ok(Ok(output)) => output,
                Ok(Err(error)) => ToolResult::failure(error.to_string()),
                Err(_) => ToolResult::failure("tool task panicked during execution"),
            };
            SessionTaskCompletion {
                id: task_id,
                output,
                cancelled: token.is_cancelled(),
            }
        });
        self.running.insert(
            id,
            RunningTask {
                cancellation,
                abort,
                lock_policy: submission.lock_policy,
            },
        );
    }

    pub(crate) fn has_completions_or_workers(&self) -> bool {
        !self.settlement_blocked && (!self.completed.is_empty() || !self.workers.is_empty())
    }

    pub(crate) fn block_settlement(&mut self) {
        self.settlement_blocked = true;
    }
    pub(crate) fn resume_settlement(&mut self) {
        self.settlement_blocked = false;
    }

    pub(crate) fn has_work(&self) -> bool {
        !self.queued.is_empty() || !self.running.is_empty() || !self.completed.is_empty()
    }

    /// A result stays retained until the owner acknowledges its terminal checkpoint.
    pub(crate) async fn next_completion(&mut self) -> Option<SessionTaskCompletion> {
        if let Some(completion) = self.completed.front() {
            return Some(completion.clone());
        }
        let completion = match self.workers.join_next_with_id().await? {
            Ok((_, completion)) => completion,
            Err(error) => {
                let id = self
                    .running
                    .iter()
                    .find(|(_, task)| task.abort.id() == error.id())
                    .map(|(id, _)| id.clone())?;
                SessionTaskCompletion {
                    id,
                    output: ToolResult::failure(format!("tool execution worker failed: {error}")),
                    cancelled: error.is_cancelled(),
                }
            }
        };
        self.completed.push_back(completion.clone());
        Some(completion)
    }

    pub(crate) fn acknowledge_completion(&mut self, id: &str) {
        self.completed_at
            .insert(id.to_owned(), tokio::time::Instant::now());
        self.completed.retain(|completion| completion.id != id);
        self.running.remove(id);
    }

    /// Returns a queued task for immediate cancellation; running work must really exit.
    pub(crate) fn cancel(&mut self, id: &str) -> Option<SessionTaskSubmission> {
        if let Some(position) = self
            .queued
            .iter()
            .position(|task| task.receipt.task_id == id)
        {
            self.completed_at
                .insert(id.to_owned(), tokio::time::Instant::now());
            return self.queued.remove(position);
        }
        if let Some(task) = self.running.get(id) {
            task.cancellation.cancel();
        }
        None
    }
}

impl Drop for SessionTaskResources {
    fn drop(&mut self) {
        for task in self.running.values() {
            task.cancellation.cancel();
        }
        // JoinSet aborts on drop. Normal close drains it first and does not use this fallback.
    }
}

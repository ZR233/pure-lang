use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::sync::MutexGuard;

use super::{CreateTaskRun, RecordTaskAgentFailure, TaskRun, TaskWorktreeOwnerSnapshot};
use crate::studio::runtime_state::StudioRecoveryIssue;
use crate::studio::store::StudioStore;
use crate::studio::{TaskRuntime, ThreadKind, ThreadRecord};
use crate::{AgentRuntimeHandle, AgentState};

mod recovery;
use recovery::resolve_worktree_recovery_groups;

mod cleanup;
mod recovery_scan;

use cleanup::WorktreeRecoveryGroup;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TaskRecoveryReport {
    pub(crate) recovered_runs: Vec<TaskRun>,
    pub(crate) issues: Vec<StudioRecoveryIssue>,
}

impl std::ops::Deref for TaskRecoveryReport {
    type Target = [TaskRun];

    fn deref(&self) -> &Self::Target {
        &self.recovered_runs
    }
}

impl IntoIterator for TaskRecoveryReport {
    type Item = TaskRun;
    type IntoIter = std::vec::IntoIter<TaskRun>;

    fn into_iter(self) -> Self::IntoIter {
        self.recovered_runs.into_iter()
    }
}

impl PartialEq<Vec<TaskRun>> for TaskRecoveryReport {
    fn eq(&self, other: &Vec<TaskRun>) -> bool {
        self.recovered_runs == *other
    }
}

/// 持久化 Task 模式事实并协调独立工作目录资源。
pub(crate) struct TaskCoordinator {
    pub(super) store: StudioStore,
    pub(super) task_runtime: TaskRuntime,
    pub(super) allocation_lock: tokio::sync::Mutex<()>,
    pub(super) branch_mutation_lock: tokio::sync::Mutex<()>,
    branch_mutation_owner: Arc<()>,
}

/// 持有期间串行化任务分支变更，并阻止 executor 基于中间 HEAD 分配。
pub(crate) struct BranchMutationGuard<'a> {
    owner: &'a Arc<()>,
    _guard: MutexGuard<'a, ()>,
}

impl TaskCoordinator {
    pub(in crate::studio) fn task_runtime(&self) -> TaskRuntime {
        self.task_runtime.clone()
    }

    pub(in crate::studio) async fn handle_agent_turn_failure(
        &self,
        input: RecordTaskAgentFailure,
        runtime: &AgentRuntimeHandle,
    ) -> Result<bool> {
        let root_thread_id = input.root_thread_id.clone();
        let source_agent_id = input.source_agent_id.clone();
        let Some(settlement) = self.task_runtime.record_agent_failure(input).await? else {
            return Ok(false);
        };
        if !settlement.terminalized {
            return Ok(false);
        }

        let root = crate::studio::agent_host::root_agent_id(&root_thread_id);
        let snapshots = runtime
            .list()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        for snapshot in snapshots {
            if snapshot.identity.id == root {
                if snapshot.identity.id.as_str() != source_agent_id
                    && let Some(turn_id) = snapshot.active_turn_id().cloned()
                {
                    runtime
                        .cancel_turn(snapshot.identity.id, turn_id)
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                }
            } else if snapshot.identity.parent_id.as_ref() == Some(&root)
                && !matches!(
                    snapshot.state,
                    AgentState::Closing(_) | AgentState::Closed(_)
                )
            {
                runtime
                    .close(snapshot.identity.id)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
        }
        Ok(true)
    }

    pub(crate) fn new(store: StudioStore, task_runtime: TaskRuntime) -> Self {
        Self {
            store,
            task_runtime,
            allocation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_owner: Arc::new(()),
        }
    }

    pub(crate) async fn lock_branch_mutation(&self) -> BranchMutationGuard<'_> {
        BranchMutationGuard {
            owner: &self.branch_mutation_owner,
            _guard: self.branch_mutation_lock.lock().await,
        }
    }

    pub(crate) fn ensure_branch_mutation_guard(
        &self,
        guard: &BranchMutationGuard<'_>,
    ) -> Result<()> {
        if Arc::ptr_eq(&self.branch_mutation_owner, guard.owner) {
            return Ok(());
        }
        bail!("branch mutation guard belongs to another coordinator")
    }

    pub(crate) async fn start_task(
        &self,
        root_thread: &ThreadRecord,
        request: &str,
        repository: impl AsRef<Path>,
    ) -> Result<TaskRun> {
        self.task_runtime.ensure_accepts_new_work()?;
        if request.trim().is_empty() {
            bail!("task request must not be empty");
        }
        if root_thread.mode != crate::StudioMode::Task
            || root_thread.thread_kind != ThreadKind::Root
        {
            bail!("task coordinator requires a task mode root Thread");
        }
        let run = self
            .task_runtime
            .create_task(CreateTaskRun {
                project_id: root_thread.project_id.clone(),
                root_thread_id: root_thread.id.clone(),
                request: request.trim().to_string(),
                workspace_root: repository.as_ref().to_string_lossy().to_string(),
            })
            .await?;
        Ok(run)
    }

    pub(crate) async fn block_continuation_failure(
        &self,
        task_run_id: &str,
        reason: String,
    ) -> Result<()> {
        let run = self
            .task_runtime
            .aggregate_for_run(task_run_id)
            .await
            .map(|aggregate| aggregate.facts.run)
            .context("task run not found while blocking continuation failure")?;
        if !run.kind().is_terminal() {
            tracing::warn!(
                task_run_id,
                reason,
                "task continuation failed; preserving main state"
            );
        }
        Ok(())
    }
}

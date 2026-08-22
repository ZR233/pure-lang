use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use tokio::sync::{MutexGuard, broadcast};

use super::recovery::{
    MergingRecovery, inspect_merging_recovery, is_retryable_merge_recovery_message,
};
use super::{
    CreateTaskRun, RecordTaskAgentFailure, TaskRun, TaskRunStateKind, TaskWorktreeOwnerSnapshot,
};
use crate::studio::ids::new_id;
use crate::studio::runtime_state::StudioRecoveryIssue;
use crate::studio::store::StudioStore;
use crate::{AgentLifecycleState, AgentRuntimeHandle};

mod recovery;
use recovery::resolve_worktree_recovery_groups;

mod cleanup;
mod leases;
mod recovery_scan;

use cleanup::WorktreeRecoveryGroup;
use leases::{
    BranchKey, acquire_process_lease, process_leases, release_process_lease,
    replace_process_lease_owner,
};

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

/// 持久化 Task 模式事实并守护用户当前分支。
pub(crate) struct TaskCoordinator {
    pub(super) store: StudioStore,
    owned_process_leases: Mutex<HashMap<BranchKey, String>>,
    pub(super) allocation_lock: tokio::sync::Mutex<()>,
    pub(super) branch_mutation_lock: tokio::sync::Mutex<()>,
    branch_mutation_owner: Arc<()>,
    terminal_fact_tx: broadcast::Sender<String>,
}

/// 持有期间串行化任务分支变更，并阻止 executor 基于中间 HEAD 分配。
pub(crate) struct BranchMutationGuard<'a> {
    owner: &'a Arc<()>,
    _guard: MutexGuard<'a, ()>,
}

impl TaskCoordinator {
    pub(in crate::studio) async fn handle_agent_turn_failure(
        &self,
        input: RecordTaskAgentFailure,
        runtime: &AgentRuntimeHandle,
    ) -> Result<bool> {
        let root_thread_id = input.root_thread_id.clone();
        let source_agent_id = input.source_agent_id.clone();
        let Some(settlement) = self.store.record_task_agent_failure(input).await? else {
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
                    && let Some(turn_id) = snapshot.active_turn_id
                {
                    runtime
                        .cancel_turn(snapshot.identity.id, turn_id)
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                }
            } else if snapshot.identity.parent_id.as_ref() == Some(&root)
                && !matches!(
                    snapshot.lifecycle,
                    AgentLifecycleState::Closing | AgentLifecycleState::Closed
                )
            {
                runtime
                    .close(snapshot.identity.id)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
        }
        self.release_owned_process_lease(&settlement.run.id);
        Ok(true)
    }

    pub(crate) fn new(store: StudioStore) -> Self {
        let (terminal_fact_tx, _) = broadcast::channel(256);
        Self {
            store,
            owned_process_leases: Mutex::new(HashMap::new()),
            allocation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_owner: Arc::new(()),
            terminal_fact_tx,
        }
    }

    pub(in crate::studio) fn subscribe_terminal_facts(&self) -> broadcast::Receiver<String> {
        self.terminal_fact_tx.subscribe()
    }

    pub(super) fn publish_terminal_fact(&self, task_run_id: &str) {
        let _ = self.terminal_fact_tx.send(task_run_id.to_string());
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

    pub(crate) async fn start_confirmed_task(
        &self,
        root_thread_id: &str,
        plan: &str,
        repository: impl AsRef<Path>,
    ) -> Result<TaskRun> {
        if plan.trim().is_empty() {
            bail!("task plan must not be empty");
        }
        let root_thread = self
            .store
            .read_thread(root_thread_id)
            .await?
            .context("task root Thread not found")?;
        let key = BranchKey::new(&root_thread.project_id);
        let owner_token = new_id("task-owner");
        acquire_process_lease(&key, &owner_token)?;

        let result = self
            .store
            .create_task_run_with_lease(CreateTaskRun {
                project_id: root_thread.project_id,
                root_thread_id: root_thread_id.to_string(),
                plan: plan.trim().to_string(),
                workspace_root: repository.as_ref().to_string_lossy().to_string(),
            })
            .await;
        let (run, _) = match result {
            Ok(result) => result,
            Err(error) => {
                release_process_lease(&key, &owner_token);
                return Err(error);
            }
        };
        replace_process_lease_owner(&key, &owner_token, &run.id)?;
        self.owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, run.id.clone());
        Ok(run)
    }

    #[cfg(test)]
    pub(crate) async fn verify_process_lease(&self, task_run_id: &str) -> Result<bool> {
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found")?;
        Ok(self.ensure_process_lease_owned(&run).is_ok())
    }

    #[cfg(test)]
    pub(crate) async fn finish_task(
        &self,
        task_run_id: &str,
        phase: TaskRunStateKind,
        status_message: Option<String>,
    ) -> Result<TaskRun> {
        if !matches!(
            phase,
            TaskRunStateKind::Completed | TaskRunStateKind::Failed | TaskRunStateKind::Cancelled
        ) {
            bail!("finish_task requires a terminal phase");
        }
        let run = self
            .store
            .transition_task_run(task_run_id, phase, status_message)
            .await?;
        self.store.release_project_lease(task_run_id).await?;
        self.release_owned_process_lease(task_run_id);
        Ok(run)
    }

    pub(crate) fn suspend(&self) {
        let mut owned = self
            .owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (key, owner) in owned.drain() {
            release_process_lease(&key, &owner);
        }
    }

    pub(crate) async fn block_continuation_failure(
        &self,
        task_run_id: &str,
        reason: String,
    ) -> Result<()> {
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found while blocking continuation failure")?;
        if !run.kind().is_terminal() {
            self.block_run(&run, reason).await?;
        }
        Ok(())
    }

    pub(super) async fn block_run(&self, run: &TaskRun, reason: String) -> Result<()> {
        let blocked = self
            .store
            .block_task_and_release_lease(&run.id, &reason)
            .await?;
        self.publish_blocked_terminal(&blocked)?;
        Ok(())
    }

    fn publish_blocked_terminal(&self, run: &TaskRun) -> Result<()> {
        if run.kind() != TaskRunStateKind::Blocked {
            bail!("blocked task fact is not canonical");
        }
        self.release_owned_process_lease(&run.id);
        release_process_lease(&BranchKey::new(&run.project_id), &run.id);
        self.publish_terminal_fact(&run.id);
        Ok(())
    }

    pub(super) fn ensure_process_lease_owned(&self, run: &TaskRun) -> Result<()> {
        let key = BranchKey::new(&run.project_id);
        let locally_owned = self
            .owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .is_some_and(|owner| owner == &run.id);
        let globally_owned = process_leases()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .is_some_and(|owner| owner == &run.id);
        if !locally_owned || !globally_owned {
            bail!("task process lease is not owned by this coordinator");
        }
        Ok(())
    }

    pub(super) fn release_owned_process_lease(&self, task_run_id: &str) {
        let mut owned = self
            .owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = owned
            .iter()
            .find_map(|(key, owner)| (owner == task_run_id).then(|| key.clone()));
        if let Some(key) = key {
            owned.remove(&key);
            release_process_lease(&key, task_run_id);
        }
    }
}

impl Drop for TaskCoordinator {
    fn drop(&mut self) {
        self.suspend();
    }
}

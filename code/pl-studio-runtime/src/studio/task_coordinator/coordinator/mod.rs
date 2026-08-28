use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures::FutureExt;
use tokio::sync::MutexGuard;

use super::{
    CreateTaskRun, RecordTaskAgentFailure, TASK_EXECUTOR_HANDOFF_SECTION_ID, TaskExecutorHandoff,
    TaskRun, TaskWorktreeOwnerSnapshot, WorkUnit,
};
use crate::studio::runtime_state::StudioRecoveryIssue;
use crate::studio::store::StudioStore;
use crate::studio::{
    InteractionEmitter, InteractionService, TaskRuntime, ThreadKind, ThreadRecord,
};
use crate::{
    AgentRuntimeHandle, AgentState, InteractionRequest, InteractionStatus, TodoListSnapshot,
};

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
    interactions: InteractionService,
    pub(super) ssh_manager: Arc<pl_core::remote::SshManager>,
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

    /// 从 root Thread 的驻留投影读取 completion gate 所需事实。
    pub(super) fn hot_completion_context(
        &self,
        root_thread_id: &str,
        runtime: &AgentRuntimeHandle,
    ) -> Result<(Vec<InteractionRequest>, Option<TodoListSnapshot>)> {
        let thread_id = pl_core::ThreadId::new(root_thread_id.to_string())?;
        let snapshot = runtime
            .thread_snapshot(&thread_id)
            .map_err(|error| anyhow::anyhow!(error))?;
        let pending = snapshot
            .interactions
            .into_iter()
            .filter(|interaction| interaction.status() == InteractionStatus::Pending)
            .collect();
        let todo = snapshot.runtime.and_then(|runtime| runtime.todo);
        Ok((pending, todo))
    }

    /// 从 executor actor 的 working state 读取类型化 handoff，不回读对象表。
    pub(super) async fn hot_work_unit_handoff(
        &self,
        runtime: &AgentRuntimeHandle,
        run: &TaskRun,
        work_unit: &WorkUnit,
    ) -> Result<Option<TaskExecutorHandoff>> {
        let Some(executor_thread_id) = work_unit.executor_thread_id.as_deref() else {
            return Ok(None);
        };
        let context = runtime
            .read_thread_context(pl_core::ThreadId::new(executor_thread_id.to_string())?)
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let sections = context
            .session
            .pinned_context_sections()
            .filter(|section| section.id.as_str() == TASK_EXECUTOR_HANDOFF_SECTION_ID)
            .collect::<Vec<_>>();
        let section = match sections.as_slice() {
            [section] => *section,
            [] => return Ok(None),
            _ => bail!("executor hot session has duplicate Task handoff sections"),
        };
        let handoff = TaskExecutorHandoff::from_context_section(section)?;
        handoff.validate_owner(run, work_unit, executor_thread_id)?;
        Ok(Some(handoff))
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

        self.settle_terminal_interactions_after_commit(&root_thread_id, runtime)
            .await;

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

    pub(crate) fn new(
        store: StudioStore,
        task_runtime: TaskRuntime,
        interactions: InteractionService,
        ssh_manager: Arc<pl_core::remote::SshManager>,
    ) -> Self {
        Self {
            store,
            task_runtime,
            interactions,
            ssh_manager,
            allocation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_owner: Arc::new(()),
        }
    }

    pub(crate) async fn settle_terminal_interactions_after_commit(
        &self,
        root_thread_id: &str,
        runtime: &AgentRuntimeHandle,
    ) {
        if let Err(error) = self
            .settle_terminal_interactions(root_thread_id, runtime)
            .await
        {
            tracing::error!(
                operation = "taskTerminalInteractionSettlement",
                root_thread_id,
                diagnostic_bytes = error.to_string().len(),
                "failed to settle pending interactions after Task terminal commit"
            );
        }
    }

    async fn settle_terminal_interactions(
        &self,
        root_thread_id: &str,
        runtime: &AgentRuntimeHandle,
    ) -> Result<()> {
        let mut thread_ids = self
            .store
            .list_threads_for_root(root_thread_id)
            .await?
            .into_iter()
            .map(|thread| thread.id)
            .collect::<Vec<_>>();
        if !thread_ids
            .iter()
            .any(|thread_id| thread_id == root_thread_id)
        {
            thread_ids.push(root_thread_id.to_string());
        }
        let snapshots = runtime
            .list()
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let mut task_owned_agents = HashSet::from([root_thread_id.to_string()]);
        loop {
            let before = task_owned_agents.len();
            for snapshot in &snapshots {
                if snapshot
                    .identity
                    .parent_id
                    .as_ref()
                    .is_some_and(|parent| task_owned_agents.contains(parent.as_str()))
                {
                    task_owned_agents.insert(snapshot.identity.id.to_string());
                }
            }
            if task_owned_agents.len() == before {
                break;
            }
        }
        thread_ids.extend(task_owned_agents);
        thread_ids.sort();
        thread_ids.dedup();

        for thread_id in thread_ids {
            let canonical_thread_id = pl_core::ThreadId::new(thread_id.clone())?;
            let snapshot = match runtime.thread_snapshot(&canonical_thread_id) {
                Ok(snapshot) => snapshot,
                Err(pl_core::AgentRuntimeError::NotFound(_)) => {
                    if self
                        .store
                        .list_pending_interactions(&thread_id)
                        .await?
                        .is_empty()
                    {
                        continue;
                    }
                    bail!("canonical Thread owner is unavailable during terminal settlement");
                }
                Err(error) => return Err(anyhow::anyhow!(error)),
            };
            if snapshot.interactions.is_empty() {
                continue;
            }
            self.interactions
                .cancel_thread(
                    snapshot.interactions,
                    "task completed",
                    terminal_interaction_emitter(runtime.clone(), thread_id.clone()),
                )
                .await
                .with_context(|| {
                    format!("failed to settle pending interactions for Thread {thread_id}")
                })?;
        }
        Ok(())
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

fn terminal_interaction_emitter(
    runtime: AgentRuntimeHandle,
    thread_id: String,
) -> InteractionEmitter {
    Arc::new(move |interaction| {
        let runtime = runtime.clone();
        let thread_id = thread_id.clone();
        async move {
            let owner_id = pl_core::ThreadId::new(thread_id.clone())?;
            runtime
                .record_thread_facts(
                    owner_id,
                    pl_core::ThreadId::new(thread_id)?,
                    vec![pl_core::ThreadNotificationFact::durable(
                        interaction.updated_at,
                        pl_protocol::ThreadNotification::InteractionChanged {
                            interaction: Box::new(interaction),
                        },
                    )],
                )
                .await?;
            Ok(())
        }
        .boxed()
    })
}

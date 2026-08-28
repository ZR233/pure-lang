//! 活动 Task 的进程内 owner 与热目录投影。
//!
//! SQLite 只在启动恢复或显式激活未驻留 Task 时读取；活动查询和业务转换始终
//! 使用这里的 `TaskAggregate`。热提交立即替换聚合并发布，SQLite 由共享 writer
//! 在后台按 owner 修订号跟随。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_core::{AgentTurnOutcome, MailboxBudgetAction};
use pl_protocol::{BudgetLimitKind, TurnOutcome, TurnRolloverOutcome};
use tokio::sync::{Mutex, RwLock};

use crate::agent::worktree::git_compatible_path;
use crate::{StudioTaskDirectoryEntry, StudioTaskRuntime};

use super::agent_host::ThreadWriteBehindWriter;
use super::task_coordinator::{
    AgentDelivery, AgentReview, AgentWorktreeDelivery, AllocateExecutor, BeginIntegratedReview,
    CreateTaskRun, DeliveryScope, ExecutorAllocation, ExecutorCloseDisposition,
    ExecutorContinuationRequest, ExecutorContinuationStateKind, ExecutorTerminalOutcome,
    IntegratedReviewTarget, MAX_EXECUTOR_BUDGET_SLICES, MergeCleanupCommand, MergeCleanupResult,
    MergeCleanupState, MergeRecord, RecordTaskAgentFailure, RecordTaskMerge, ReviewFileCoverage,
    ReviewPassedOutcome, ReviewRoundCommand, ReviewRoundRecord, ReviewRoundState,
    ReviewRoundStateKind, ReviewScope, ReviewVerdict, TaskCommand, TaskContext, TaskFailureKind,
    TaskIssueCommand, TaskIssueDisposition, TaskIssueRecord, TaskIssueSettlement, TaskIssueState,
    TaskMergeScope, TaskOutcome, TaskPlan, TaskPlannerWakeRequest, TaskPlannerWakeSource, TaskRun,
    TaskRunState, TaskRunStateKind, TaskStopOrigin, TaskStopReason, TaskWorktreeDisposition,
    WaitingReviewPhase, WorkCompletionCommand, WorkCompletionContent, WorkCompletionKind,
    WorkCompletionRecord, WorkCompletionState, WorkCompletionStatus, WorkUnit, WorkUnitCommand,
    WorkUnitContext, WorkUnitState, WorkUnitStateKind,
};
use super::task_persistence::{
    TaskPersistenceCommit, TaskStopEventFact, load_task_commit_revision,
};
use super::{ProductEventBus, StudioStore, task_projection};

mod executor;
mod facts;
mod lifecycle;
mod merge;
mod review;
#[cfg(test)]
mod tests;

use facts::*;
#[derive(Debug, Clone)]
pub(crate) struct TaskAggregate {
    pub(crate) entry: StudioTaskDirectoryEntry,
    /// Task owner 的完整领域事实；活动协调与工具查询必须读取这里。
    pub(crate) facts: task_projection::LoadedTaskAggregate,
    /// owner 已提交并对热消费者可见的修订号。
    pub(crate) hot_revision: u64,
    /// 最近一次从 SQLite 恢复或确认的修订号。
    pub(crate) durable_revision: u64,
    /// 已由 ThreadActor 热提交接收的计划者唤醒邮件。
    ///
    /// 这是跨 owner 的进程内投影：运行期间以此集合去重，冷启动时才从
    /// SQLite 中已经持久化的 Thread mailbox 事实重建。
    delivered_planner_wakes: HashSet<String>,
}

#[derive(Clone)]
pub(crate) struct TaskRuntime {
    store: StudioStore,
    product_events: ProductEventBus,
    aggregates: Arc<RwLock<HashMap<String, TaskAggregate>>>,
    owners: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    writer: ThreadWriteBehindWriter,
}

pub(super) struct ResolveTaskIssue<'a> {
    pub(super) issue_id: &'a str,
    pub(super) operation_id: &'a str,
    pub(super) summary: &'a str,
    pub(super) evidence: &'a str,
    pub(super) expected_revision: u64,
    pub(super) expected_generation: u64,
}

impl TaskRuntime {
    /// 测试构造：独立 writer 实例即可。
    #[cfg(test)]
    pub(crate) fn new(store: StudioStore, product_events: ProductEventBus) -> Self {
        let writer = ThreadWriteBehindWriter::new(store.clone());
        Self::with_writer(store, product_events, writer)
    }

    /// ThreadRepository、ProductEventBus 与 TaskRuntime 必须共享同一进程级 writer。
    pub(in crate::studio) fn with_writer(
        store: StudioStore,
        product_events: ProductEventBus,
        writer: ThreadWriteBehindWriter,
    ) -> Self {
        Self {
            store,
            product_events,
            aggregates: Arc::new(RwLock::new(HashMap::new())),
            owners: Arc::new(Mutex::new(HashMap::new())),
            writer,
        }
    }

    /// ThreadRepository 与 TaskRuntime 必须共享同一个进程级 writer。
    #[cfg(test)]
    pub(in crate::studio) fn writer(&self) -> ThreadWriteBehindWriter {
        self.writer.clone()
    }

    pub(crate) fn ensure_accepts_new_work(&self) -> Result<()> {
        if self
            .product_events
            .persistence_state()
            .state
            .accepts_new_work()
        {
            return Ok(());
        }
        anyhow::bail!(
            "Studio persistence is unavailable; new Task work is paused until pending facts are durable"
        )
    }

    /// 从 SQLite 冷基线恢复活动 Task；此方法只在 Studio 启动时调用。
    ///
    /// 只装载非终态聚合（终态 Task 是冷数据，显式访问时经 `activate` 冷激活），
    /// 与恢复扫描共用 `list_active_task_runs` 的同一份活动快照。
    pub(crate) async fn initialize(&self, active_runs: Vec<TaskRun>) -> Result<()> {
        let active_roots = active_runs
            .iter()
            .map(|run| run.root_thread_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut restored = Vec::new();
        for root_thread_id in active_roots {
            if let Some(facts) =
                task_projection::load_task_aggregate(&self.store, &root_thread_id).await?
            {
                let mut delivered_planner_wakes = HashSet::new();
                for wake in planner_wakes_for_facts(&facts)? {
                    if self.store.task_planner_wake_was_delivered(&wake).await? {
                        delivered_planner_wakes.insert(wake.mail_id());
                    }
                }
                let entry = StudioTaskDirectoryEntry {
                    root_thread_id,
                    task: facts.runtime.clone(),
                };
                restored.push((entry, facts, delivered_planner_wakes));
            }
        }
        restored
            .sort_by(|(left, _, _), (right, _, _)| left.root_thread_id.cmp(&right.root_thread_id));
        let entries = restored
            .iter()
            .map(|(entry, _, _)| entry.clone())
            .collect::<Vec<_>>();
        let mut aggregates = self.aggregates.write().await;
        aggregates.clear();
        for (entry, facts, delivered_planner_wakes) in restored {
            let revision = load_task_commit_revision(self.store.database(), &entry.root_thread_id)
                .await?
                .unwrap_or(0);
            self.writer
                .seed_task_durable_revision(&entry.root_thread_id, revision);
            if !facts.run.kind().is_terminal() {
                self.writer
                    .seed_task_lifecycle(&entry.root_thread_id, &facts.run.id);
            }
            aggregates.insert(
                entry.root_thread_id.clone(),
                TaskAggregate {
                    entry,
                    facts,
                    hot_revision: revision,
                    durable_revision: revision,
                    delivered_planner_wakes,
                },
            );
        }
        drop(aggregates);
        self.product_events.initialize_task_directory(entries).await;
        Ok(())
    }

    /// 显式激活未驻留 Task；这是启动恢复之外唯一允许的 SQLite 冷读取入口。
    pub(crate) async fn activate(&self, root_thread_id: &str) -> Result<Option<TaskAggregate>> {
        if let Some(aggregate) = self.aggregate(root_thread_id).await {
            return Ok(Some(aggregate));
        }
        let owner = self.owner(root_thread_id).await;
        let _guard = owner.lock().await;
        if let Some(aggregate) = self.aggregate(root_thread_id).await {
            return Ok(Some(aggregate));
        }
        let Some(facts) = task_projection::load_task_aggregate(&self.store, root_thread_id).await?
        else {
            return Ok(None);
        };
        let mut delivered_planner_wakes = HashSet::new();
        for wake in planner_wakes_for_facts(&facts)? {
            if self.store.task_planner_wake_was_delivered(&wake).await? {
                delivered_planner_wakes.insert(wake.mail_id());
            }
        }
        let entry = StudioTaskDirectoryEntry {
            root_thread_id: root_thread_id.to_string(),
            task: facts.runtime.clone(),
        };
        let revision = load_task_commit_revision(self.store.database(), root_thread_id)
            .await?
            .unwrap_or(0);
        self.writer
            .seed_task_durable_revision(root_thread_id, revision);
        if !facts.run.kind().is_terminal() {
            self.writer
                .seed_task_lifecycle(root_thread_id, &facts.run.id);
        }
        let aggregate = TaskAggregate {
            entry: entry.clone(),
            facts,
            hot_revision: revision,
            durable_revision: revision,
            delivered_planner_wakes,
        };
        self.aggregates
            .write()
            .await
            .insert(root_thread_id.to_string(), aggregate.clone());
        self.product_events.apply_task_entry(entry).await;
        Ok(Some(aggregate))
    }

    /// 仅淘汰已经终态且全部热修订均已耐久化的 Task 聚合。
    /// Task 目录条目保留，后续显式访问可再次冷激活。
    pub(crate) async fn evict_durable(&self, root_thread_id: &str) -> bool {
        let owner = self.owner(root_thread_id).await;
        let _guard = owner.lock().await;
        let mut aggregates = self.aggregates.write().await;
        let Some(aggregate) = aggregates.get_mut(root_thread_id) else {
            return true;
        };
        if let Some(revision) = self.writer.task_durable_revision(root_thread_id) {
            aggregate.durable_revision = aggregate.durable_revision.max(revision);
        }
        if !aggregate.facts.run.kind().is_terminal()
            || aggregate.durable_revision < aggregate.hot_revision
        {
            return false;
        }
        aggregates.remove(root_thread_id);
        true
    }

    /// 读取已经驻留的热 Task，不触发 SQLite。
    pub(crate) async fn snapshot(&self, root_thread_id: &str) -> Option<StudioTaskRuntime> {
        self.aggregates
            .read()
            .await
            .get(root_thread_id)
            .map(|aggregate| {
                debug_assert!(aggregate.durable_revision <= aggregate.hot_revision);
                aggregate.entry.task.clone()
            })
    }

    /// 读取完整活动聚合，不触发 SQLite。
    pub(crate) async fn aggregate(&self, root_thread_id: &str) -> Option<TaskAggregate> {
        let mut aggregate = self.aggregates.read().await.get(root_thread_id).cloned()?;
        if let Some(revision) = self.writer.task_durable_revision(root_thread_id) {
            aggregate.durable_revision = aggregate.durable_revision.max(revision);
        }
        Some(aggregate)
    }

    /// 由 TaskRun 标识读取驻留聚合；用于 review/executor 等子资源回查 owner。
    pub(crate) async fn aggregate_for_run(&self, task_run_id: &str) -> Option<TaskAggregate> {
        let root_thread_id =
            self.aggregates
                .read()
                .await
                .iter()
                .find_map(|(root_thread_id, aggregate)| {
                    (aggregate.facts.run.id == task_run_id).then(|| root_thread_id.clone())
                })?;
        self.aggregate(&root_thread_id).await
    }

    /// 在指定 Task owner 的串行临界区内计算并提交完整热事实。
    ///
    /// 领域协调器可用此入口组合 WorkUnit、Completion、Review、Issue 与 Merge 的
    /// 同聚合转换；闭包不得执行 IO。成功返回前事实已经进入内存目录，SQLite 仅在
    /// 后台跟随。
    pub(crate) async fn commit_facts<F>(
        &self,
        root_thread_id: &str,
        decide: F,
    ) -> Result<task_projection::LoadedTaskAggregate>
    where
        F: FnOnce(
            &task_projection::LoadedTaskAggregate,
        ) -> Result<task_projection::LoadedTaskAggregate>,
    {
        self.commit_hot(root_thread_id, move |current| {
            decide(current.context("active Task aggregate is not resident")?)
        })
        .await
    }

    pub(crate) async fn root_for_work_unit(&self, work_unit_id: &str) -> Option<String> {
        self.aggregates
            .read()
            .await
            .iter()
            .find_map(|(root_thread_id, aggregate)| {
                aggregate
                    .facts
                    .work_units
                    .iter()
                    .any(|unit| unit.id == work_unit_id)
                    .then(|| root_thread_id.clone())
            })
    }

    pub(crate) async fn root_for_review(&self, review_round_id: &str) -> Option<String> {
        self.aggregates
            .read()
            .await
            .iter()
            .find_map(|(root_thread_id, aggregate)| {
                aggregate
                    .facts
                    .reviews
                    .iter()
                    .any(|round| round.id == review_round_id)
                    .then(|| root_thread_id.clone())
            })
    }

    /// 只从驻留 Task 聚合计算尚未投递的计划者唤醒，不读取 SQLite。
    pub(crate) async fn pending_planner_wakes(
        &self,
        root_thread_id: Option<&str>,
    ) -> Result<Vec<TaskPlannerWakeRequest>> {
        let aggregates = self.aggregates.read().await;
        let mut wakes = Vec::new();
        for (root, aggregate) in aggregates.iter() {
            if root_thread_id.is_some_and(|expected| expected != root) {
                continue;
            }
            wakes.extend(
                planner_wakes_for_facts(&aggregate.facts)?
                    .into_iter()
                    .filter(|wake| !aggregate.delivered_planner_wakes.contains(&wake.mail_id())),
            );
        }
        Ok(wakes)
    }

    /// ThreadActor 已接收稳定 mail id 后，同步推进 Task owner 的热去重投影。
    ///
    /// mailbox 本身由 Thread owner 异步持久化；这里不创建第二份 SQLite 事实。
    pub(crate) async fn mark_planner_wake_delivered(
        &self,
        wake: &TaskPlannerWakeRequest,
    ) -> Result<()> {
        let owner = self.owner(&wake.root_thread_id).await;
        let _guard = owner.lock().await;
        let mut aggregates = self.aggregates.write().await;
        let aggregate = aggregates
            .get_mut(&wake.root_thread_id)
            .context("Task Planner wake owner is not resident")?;
        anyhow::ensure!(
            aggregate.facts.run.id == wake.task_run_id,
            "Task Planner wake belongs to a stale TaskRun"
        );
        let mail_id = wake.mail_id();
        anyhow::ensure!(
            planner_wakes_for_facts(&aggregate.facts)?
                .into_iter()
                .any(|candidate| candidate.mail_id() == mail_id),
            "Task Planner wake no longer matches a pending Task fact"
        );
        aggregate.delivered_planner_wakes.insert(mail_id);
        Ok(())
    }

    /// 显式 Task 耐久化屏障，只供关机、淘汰和不可逆外部动作使用。
    #[allow(dead_code)]
    pub(crate) async fn await_durable(&self, root_thread_id: &str, revision: u64) -> Result<()> {
        self.writer
            .await_task_durable(root_thread_id, revision)
            .await
            .map_err(anyhow::Error::msg)?;
        if let Some(aggregate) = self.aggregates.write().await.get_mut(root_thread_id) {
            aggregate.durable_revision = aggregate.durable_revision.max(revision);
        }
        Ok(())
    }

    async fn commit_hot<F>(
        &self,
        root_thread_id: &str,
        decide: F,
    ) -> Result<task_projection::LoadedTaskAggregate>
    where
        F: FnOnce(
            Option<&task_projection::LoadedTaskAggregate>,
        ) -> Result<task_projection::LoadedTaskAggregate>,
    {
        self.commit_hot_with_stop_events(root_thread_id, Vec::new(), decide)
            .await
    }

    async fn commit_hot_with_stop_events<F>(
        &self,
        root_thread_id: &str,
        stop_events: Vec<TaskStopEventFact>,
        decide: F,
    ) -> Result<task_projection::LoadedTaskAggregate>
    where
        F: FnOnce(
            Option<&task_projection::LoadedTaskAggregate>,
        ) -> Result<task_projection::LoadedTaskAggregate>,
    {
        let owner = self.owner(root_thread_id).await;
        let _guard = owner.lock().await;
        let current = self.aggregates.read().await.get(root_thread_id).cloned();
        let mut facts = decide(current.as_ref().map(|aggregate| &aggregate.facts))?;
        anyhow::ensure!(
            facts.run.root_thread_id == root_thread_id,
            "Task aggregate owner does not match root Thread"
        );
        for stop_event in &stop_events {
            if let Some(existing) = facts
                .stop_events
                .iter()
                .find(|existing| existing.id == stop_event.id)
            {
                anyhow::ensure!(
                    existing == stop_event,
                    "Task stop event id is already bound to different hot facts"
                );
            } else {
                facts.stop_events.push(stop_event.clone());
            }
        }
        if let Some(current) = current.as_ref()
            && current.facts.run.id == facts.run.id
        {
            if same_task_domain_facts(&current.facts, &facts) && stop_events.is_empty() {
                return Ok(current.facts.clone());
            }
            let next_run_revision = current
                .facts
                .run
                .revision
                .checked_add(1)
                .context("TaskRun revision overflow")?;
            if facts.run.revision == current.facts.run.revision {
                facts.run.revision = next_run_revision;
            } else {
                anyhow::ensure!(
                    facts.run.revision == next_run_revision,
                    "Task owner commit must advance TaskRun revision exactly once"
                );
            }
            facts.run.updated_at = super::ids::unix_seconds();
        } else {
            anyhow::ensure!(
                facts.run.revision == 0,
                "a new TaskRun lifecycle must start at revision zero"
            );
        }
        facts.refresh_projection()?;
        let expected_owner_revision = current.as_ref().map_or(0, |value| value.hot_revision);
        let revision = expected_owner_revision
            .checked_add(1)
            .context("Task owner revision overflow")?;
        let expected_run_revision = current.as_ref().and_then(|aggregate| {
            (aggregate.facts.run.id == facts.run.id).then_some(aggregate.facts.run.revision)
        });
        let persistence_commit = TaskPersistenceCommit {
            owner_id: root_thread_id.to_string(),
            expected_owner_revision,
            revision,
            expected_run_revision,
            aggregate: facts.clone(),
            stop_events,
        };
        let entry = StudioTaskDirectoryEntry {
            root_thread_id: root_thread_id.to_string(),
            task: facts.runtime.clone(),
        };
        let durable_revision = current.as_ref().map_or(0, |value| value.durable_revision);
        let delivered_planner_wakes = current
            .as_ref()
            .filter(|value| value.facts.run.id == facts.run.id)
            .map_or_else(HashSet::new, |value| value.delivered_planner_wakes.clone());
        self.writer
            .accept_task(persistence_commit)
            .map_err(anyhow::Error::msg)?;
        self.aggregates.write().await.insert(
            root_thread_id.to_string(),
            TaskAggregate {
                entry: entry.clone(),
                facts: facts.clone(),
                hot_revision: revision,
                durable_revision,
                delivered_planner_wakes,
            },
        );
        self.product_events.apply_task_entry(entry).await;
        Ok(facts)
    }

    pub(crate) async fn has_active_task(&self, root_thread_id: &str) -> bool {
        self.snapshot(root_thread_id)
            .await
            .is_some_and(|task| !matches!(task.state, crate::StudioTaskState::Completed(_)))
    }

    pub(crate) async fn has_any_active_task(&self) -> bool {
        self.aggregates.read().await.values().any(|aggregate| {
            !matches!(
                aggregate.entry.task.state,
                crate::StudioTaskState::Completed(_)
            )
        })
    }

    pub(crate) async fn has_active_task_for_roots(&self, root_thread_ids: &[String]) -> bool {
        let aggregates = self.aggregates.read().await;
        root_thread_ids.iter().any(|root_thread_id| {
            aggregates.get(root_thread_id).is_some_and(|aggregate| {
                !matches!(
                    aggregate.entry.task.state,
                    crate::StudioTaskState::Completed(_)
                )
            })
        })
    }

    /// 返回活动 Task 当前引用的根、执行者和审查者 Thread。
    ///
    /// 启动恢复用该集合先恢复 actor；这是驻留钉住来源，不触发 SQLite 冷查询。
    pub(crate) async fn active_thread_ids(&self) -> Vec<String> {
        let aggregates = self.aggregates.read().await;
        let mut thread_ids = Vec::new();
        for aggregate in aggregates.values() {
            if matches!(
                aggregate.entry.task.state,
                crate::StudioTaskState::Completed(_)
            ) {
                continue;
            }
            thread_ids.push(aggregate.entry.root_thread_id.clone());
            thread_ids.extend(
                aggregate
                    .facts
                    .work_units
                    .iter()
                    .filter_map(|unit| unit.executor_thread_id.clone()),
            );
            thread_ids.extend(
                aggregate
                    .facts
                    .reviews
                    .iter()
                    .filter_map(|round| round.reviewer_thread_id().map(ToOwned::to_owned)),
            );
        }
        thread_ids.sort();
        thread_ids.dedup();
        thread_ids
    }

    async fn owner(&self, root_thread_id: &str) -> Arc<Mutex<()>> {
        self.owners
            .lock()
            .await
            .entry(root_thread_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

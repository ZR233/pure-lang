//! 重启后活动 Task 的恢复扫描与恢复问题收集。

use anyhow::{Context, Result, bail};
use std::collections::HashSet;

use super::leases::{BranchKey, acquire_process_lease, release_process_lease};
use super::{
    MergingRecovery, TaskCoordinator, TaskRecoveryReport, TaskRun, TaskRunStateKind,
    inspect_merging_recovery, is_retryable_merge_recovery_message,
    resolve_worktree_recovery_groups,
};

use crate::studio::runtime_state::{
    StudioRecoveryIssue, StudioRecoveryIssueAction, StudioRecoveryIssueCategory,
    StudioRecoveryIssueScope,
};

impl TaskCoordinator {
    pub(crate) async fn recover_active_tasks(&self) -> Result<TaskRecoveryReport> {
        let mut report = TaskRecoveryReport::default();
        let mut prepared = Vec::new();
        let mut failed_agent_runs = HashSet::new();
        for run in self.store.list_active_task_runs().await? {
            let key = BranchKey::new(&run.project_id);
            if let Err(error) = acquire_process_lease(&key, &run.id) {
                let message = error.to_string();
                self.block_run(&run, message.clone()).await?;
                self.push_recovery_issue(
                    &mut report,
                    &run,
                    StudioRecoveryIssueScope::Thread,
                    StudioRecoveryIssueCategory::ProcessLease,
                    StudioRecoveryIssueAction::CleanupThread,
                    message,
                )
                .await?;
                continue;
            }
            self.owned_process_leases
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(key, run.id.clone());
            if let Err(error) = self
                .store
                .reconcile_task_agents_after_restart(&run.id)
                .await
            {
                let message = format!("agent restart reconciliation failed: {error}");
                self.block_run(&run, message.clone()).await?;
                self.push_recovery_issue(
                    &mut report,
                    &run,
                    StudioRecoveryIssueScope::Thread,
                    StudioRecoveryIssueCategory::AgentState,
                    StudioRecoveryIssueAction::CleanupThread,
                    message,
                )
                .await?;
                failed_agent_runs.insert(run.id.clone());
                continue;
            }
            let run = self
                .store
                .read_task_run(&run.id)
                .await?
                .context("task run disappeared after agent restart reconciliation")?;
            if run.kind() == TaskRunStateKind::Stopping {
                let reason = run
                    .stop_reason()
                    .map_or("task stop resumed after restart", |reason| reason.as_str());
                if let Err(error) = self
                    .store
                    .settle_agents_for_task_stop(&run.id, run.generation(), reason)
                    .await
                {
                    let message = format!("task stop recovery settlement failed: {error}");
                    self.block_run(&run, message.clone()).await?;
                    self.push_recovery_issue(
                        &mut report,
                        &run,
                        StudioRecoveryIssueScope::Thread,
                        StudioRecoveryIssueCategory::AgentState,
                        StudioRecoveryIssueAction::CleanupThread,
                        message,
                    )
                    .await?;
                    failed_agent_runs.insert(run.id.clone());
                    continue;
                }
            }
            prepared.push(run);
        }

        let owners = self.store.list_all_task_worktree_owners().await?;
        let preflight = resolve_worktree_recovery_groups(owners).await;
        let mut failed_preflight_runs = HashSet::new();
        for failure in preflight.failures {
            for run in &failure.runs {
                failed_preflight_runs.insert(run.id.clone());
                if !run.kind().is_terminal() {
                    self.block_run(run, failure.message.clone()).await?;
                }
                self.push_recovery_issue(
                    &mut report,
                    run,
                    StudioRecoveryIssueScope::Project,
                    StudioRecoveryIssueCategory::Repository,
                    StudioRecoveryIssueAction::RemoveProject,
                    failure.message.clone(),
                )
                .await?;
            }
        }
        let groups = preflight.groups;
        let run_groups = preflight.run_groups;
        let skipped_groups = failed_agent_runs
            .iter()
            .filter_map(|run_id| run_groups.get(run_id).cloned())
            .collect::<HashSet<_>>();
        let mut successful_groups = HashSet::new();
        for (key, group) in groups {
            if skipped_groups.contains(&key) {
                continue;
            }
            if group.owners.iter().all(|owner| owner.resources.is_empty()) {
                successful_groups.insert(key);
                continue;
            }
            if let Err(error) = self
                .reconcile_durable_worktrees(&group.repositories, &group.owners)
                .await
            {
                let message = format!("worktree restart reconciliation failed: {error}");
                let affected = prepared
                    .iter()
                    .filter(|run| run_groups.get(&run.id) == Some(&key))
                    .cloned()
                    .collect::<Vec<_>>();
                for run in &affected {
                    self.block_run(run, message.clone()).await?;
                }
                for owner in &group.owners {
                    self.push_recovery_issue(
                        &mut report,
                        &owner.run,
                        StudioRecoveryIssueScope::Thread,
                        StudioRecoveryIssueCategory::Worktree,
                        StudioRecoveryIssueAction::CleanupThread,
                        message.clone(),
                    )
                    .await?;
                }
                continue;
            }
            successful_groups.insert(key);
        }

        for run in prepared {
            if failed_preflight_runs.contains(&run.id) {
                continue;
            }
            if !run_groups
                .get(&run.id)
                .is_some_and(|group| successful_groups.contains(group))
            {
                continue;
            }
            if run.kind() == TaskRunStateKind::Merging {
                match inspect_merging_recovery(&run).await {
                    MergingRecovery::Resume => report.recovered_runs.push(run),
                }
                continue;
            }
            if run.kind() == TaskRunStateKind::Stopping {
                let reason = run
                    .stop_reason()
                    .map_or("task stop resumed after restart", |reason| reason.as_str());
                let guard = self.lock_branch_mutation().await;
                if let Err(error) = self
                    .stop_task_locked(&run.id, run.generation(), reason, &guard)
                    .await
                {
                    drop(guard);
                    let message = format!("task stop recovery failed: {error}");
                    self.block_run(&run, message.clone()).await?;
                    self.push_recovery_issue(
                        &mut report,
                        &run,
                        StudioRecoveryIssueScope::Thread,
                        StudioRecoveryIssueCategory::AgentState,
                        StudioRecoveryIssueAction::CleanupThread,
                        message,
                    )
                    .await?;
                }
                continue;
            }
            report.recovered_runs.push(run);
        }
        for run in self.store.list_retryable_blocked_merge_task_runs().await? {
            let message = run
                .status_message()
                .map(str::to_string)
                .context("retryable merge recovery task is missing its diagnostic")?;
            self.push_recovery_issue(
                &mut report,
                &run,
                StudioRecoveryIssueScope::Thread,
                StudioRecoveryIssueCategory::Merge,
                StudioRecoveryIssueAction::Retry,
                message,
            )
            .await?;
        }
        Ok(report)
    }

    pub(crate) async fn retry_recovery_issue(
        &self,
        issue: &StudioRecoveryIssue,
    ) -> Result<TaskRun> {
        if issue.action != StudioRecoveryIssueAction::Retry
            || issue.category != StudioRecoveryIssueCategory::Merge
        {
            bail!("recovery issue does not authorize merge reconciliation");
        }
        let task_run_id = issue
            .task_run_id
            .as_deref()
            .context("merge recovery issue has no task run")?;
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("merge recovery task run not found")?;
        if run.kind() != TaskRunStateKind::Blocked
            || run.root_thread_id != issue.thread_id.as_deref().unwrap_or_default()
            || run.status_message() != Some(issue.message.as_str())
            || !is_retryable_merge_recovery_message(&issue.message)
        {
            bail!("merge recovery state changed before retry");
        }
        let key = BranchKey::new(&run.project_id);
        acquire_process_lease(&key, &run.id)?;
        let retried = match self.store.retry_blocked_merge_task(&run).await {
            Ok(retried) => retried,
            Err(error) => {
                release_process_lease(&key, &run.id);
                return Err(error);
            }
        };
        self.owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, retried.id.clone());
        Ok(retried)
    }

    async fn push_recovery_issue(
        &self,
        report: &mut TaskRecoveryReport,
        run: &TaskRun,
        scope: StudioRecoveryIssueScope,
        category: StudioRecoveryIssueCategory,
        action: StudioRecoveryIssueAction,
        message: String,
    ) -> Result<()> {
        let session = self.store.read_thread(&run.root_thread_id).await?;
        let project_id = session.as_ref().map(|session| session.project_id.clone());
        let category_key = match category {
            StudioRecoveryIssueCategory::ProcessLease => "process-lease",
            StudioRecoveryIssueCategory::AgentState => "agent-state",
            StudioRecoveryIssueCategory::Worktree => "worktree",
            StudioRecoveryIssueCategory::Repository => "repository",
            StudioRecoveryIssueCategory::Merge => "merge",
            StudioRecoveryIssueCategory::Conflict => "conflict",
        };
        let id = format!("recovery-issue-{category_key}-{}", run.id);
        if report.issues.iter().any(|issue| issue.id == id) {
            return Ok(());
        }
        report.issues.push(StudioRecoveryIssue {
            id,
            scope,
            category,
            action,
            project_id,
            thread_id: Some(run.root_thread_id.clone()),
            task_run_id: Some(run.id.clone()),
            message,
        });
        Ok(())
    }
}

//! 重启后恢复活动任务的模型执行与独立资源；恢复问题不改变主任务状态。

use anyhow::{Context, Result, bail};
use std::collections::HashSet;

use super::{TaskCoordinator, TaskRecoveryReport, TaskRun, resolve_worktree_recovery_groups};
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
            if let Err(error) = self
                .store
                .reconcile_task_agents_after_restart(&run.id)
                .await
            {
                let message = format!("agent restart reconciliation failed: {error}");
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
            prepared.push(
                self.store
                    .read_task_run(&run.id)
                    .await?
                    .context("task run disappeared after restart reconciliation")?,
            );
        }

        let owners = self.store.list_all_task_worktree_owners().await?;
        let preflight = resolve_worktree_recovery_groups(owners).await;
        let mut failed_preflight_runs = HashSet::new();
        for failure in preflight.failures {
            for run in &failure.runs {
                failed_preflight_runs.insert(run.id.clone());
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

        let run_groups = preflight.run_groups;
        let mut successful_groups = HashSet::new();
        for (key, group) in preflight.groups {
            if group
                .owners
                .iter()
                .any(|owner| failed_agent_runs.contains(&owner.run.id))
            {
                continue;
            }
            if group.owners.iter().all(|owner| owner.resources.is_empty()) {
                successful_groups.insert(key);
                continue;
            }
            match self
                .reconcile_durable_worktrees(&group.repositories, &group.owners)
                .await
            {
                Ok(()) => {
                    successful_groups.insert(key);
                }
                Err(error) => {
                    let message = format!("worktree restart reconciliation failed: {error}");
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
                }
            }
        }

        report
            .recovered_runs
            .extend(prepared.into_iter().filter(|run| {
                !failed_preflight_runs.contains(&run.id)
                    && run_groups
                        .get(&run.id)
                        .is_none_or(|group| successful_groups.contains(group))
            }));
        Ok(report)
    }

    pub(crate) async fn retry_recovery_issue(
        &self,
        issue: &StudioRecoveryIssue,
    ) -> Result<TaskRun> {
        let _ = issue;
        bail!("resource issues are resolved through task_transition.resolveIssue")
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

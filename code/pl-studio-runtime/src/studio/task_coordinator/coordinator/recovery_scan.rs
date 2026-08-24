//! 重启后恢复活动任务的模型执行与独立资源；恢复问题不改变主任务状态。

use anyhow::{Context, Result, bail};
use std::collections::HashSet;

use super::{
    RecordTaskAgentFailure, TaskCoordinator, TaskRecoveryReport, TaskRun,
    resolve_worktree_recovery_groups,
};
use crate::studio::agent_host::StudioAgentRepository;
use crate::studio::runtime_state::{
    StudioRecoveryIssue, StudioRecoveryIssueAction, StudioRecoveryIssueCategory,
    StudioRecoveryIssueScope,
};
use crate::studio::task_coordinator::TaskIssueDisposition;

impl TaskCoordinator {
    pub(crate) async fn recover_active_tasks(&self) -> Result<TaskRecoveryReport> {
        let mut report = TaskRecoveryReport::default();
        let mut prepared = Vec::new();
        let mut failed_agent_runs = HashSet::new();
        let invalid_session_roots = StudioAgentRepository::for_reads(self.store.clone())
            .audit_registered_sessions()
            .await?
            .into_iter()
            .map(|failure| failure.root_thread_id)
            .collect::<HashSet<_>>();
        for run in self.store.list_active_task_runs().await? {
            if !invalid_session_roots.contains(&run.root_thread_id)
                && self.settle_known_legacy_root_fault(&run).await?
            {
                continue;
            }
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

    /// 结算旧版本 reasoning 分块回归留下的 `Faulted root + active TaskRun`。
    ///
    /// 来源 Turn id 同时是 task issue 的幂等键；重复启动不会重复创建 issue 或完成任务。
    async fn settle_known_legacy_root_fault(&self, run: &TaskRun) -> Result<bool> {
        const LEGACY_REASONING_CHUNK_FAULT: &str = "chunk index skipped an earlier chunk";

        let Some(thread) = self.store.read_thread(&run.root_thread_id).await? else {
            return Ok(false);
        };
        let known_fault = thread.status == pl_protocol::ThreadStatus::Faulted
            && thread
                .error
                .as_deref()
                .is_some_and(|error| error.contains(LEGACY_REASONING_CHUNK_FAULT));
        if !known_fault {
            return Ok(false);
        }
        let history = self
            .store
            .list_thread_turns(&run.root_thread_id, None, 1)
            .await?;
        let Some(turn) = history.turns.first().map(|entry| &entry.turn) else {
            return Ok(false);
        };
        let Some(failure) = turn.failure().cloned() else {
            return Ok(false);
        };
        if !failure.message.contains(LEGACY_REASONING_CHUNK_FAULT) {
            return Ok(false);
        }

        let _ = self
            .store
            .record_task_agent_failure(RecordTaskAgentFailure {
                root_thread_id: run.root_thread_id.clone(),
                source_thread_id: run.root_thread_id.clone(),
                source_turn_id: turn.id.clone(),
                source_agent_id: thread.agent_path,
                source_role: thread.role,
                failure,
                disposition: TaskIssueDisposition::Fatal,
            })
            .await?;
        Ok(self
            .store
            .read_task_run(&run.id)
            .await?
            .is_some_and(|run| run.kind().is_terminal()))
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

#[cfg(test)]
mod tests {
    use pl_core::{AgentState, FaultedAgentState};
    use pl_model::TokenUsage;
    use pl_protocol::{FailedTurnState, StateError, TurnFailure, TurnFailureCategory, TurnState};
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, IntoActiveModel};

    use super::*;
    use crate::studio::entity::{thread, turn};
    use crate::studio::task_coordinator::CreateTaskRun;
    use crate::{StudioMode, StudioStore};

    #[tokio::test]
    async fn known_faulted_root_terminalizes_planning_task_idempotently() {
        const DETAIL: &str = "chunk index skipped an earlier chunk";

        let store = StudioStore::open_memory().await.unwrap();
        let workspace =
            std::env::temp_dir().join(format!("pure-legacy-fault-recovery-{}", std::process::id()));
        std::fs::create_dir_all(&workspace).unwrap();
        let project = store.upsert_project(&workspace).await.unwrap();
        let root = store
            .create_thread(&project.id, "Legacy fault", StudioMode::Task)
            .await
            .unwrap();
        let run = store
            .create_task_run(CreateTaskRun {
                project_id: project.id,
                root_thread_id: root.id.clone(),
                request: "continue".to_string(),
                workspace_root: workspace.to_string_lossy().into_owned(),
            })
            .await
            .unwrap();

        let faulted = AgentState::Faulted(FaultedAgentState::new(
            StateError {
                code: "agentRuntimeFault".to_string(),
                message: format!("thread events failed: {DETAIL}"),
                retryable: false,
            },
            Some(pl_core::TurnId::new("turn-legacy").unwrap()),
        ));
        let row = thread::Entity::find_by_id(&root.id)
            .one(store.database())
            .await
            .unwrap()
            .unwrap();
        let mut active = row.into_active_model();
        active.state_json = Set(serde_json::to_string(&faulted).unwrap());
        active.update(store.database()).await.unwrap();

        let failure = TurnFailure::permanent(
            TurnFailureCategory::Internal,
            format!("thread projection invariant failed: {DETAIL}"),
        );
        turn::ActiveModel {
            id: Set("turn-legacy".to_string()),
            thread_id: Set(root.id.clone()),
            ordinal: Set(0),
            revision: Set(1),
            state_json: Set(
                serde_json::to_string(&TurnState::Failed(FailedTurnState::new(
                    Some(1),
                    2,
                    failure,
                )))
                .unwrap(),
            ),
            model_json: Set(None),
            usage_json: Set(serde_json::to_string(&TokenUsage::default()).unwrap()),
            metadata_json: Set(None),
            updated_at: Set(2),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .unwrap();

        let product_events = crate::studio::ProductEventBus::new(store.clone());
        let task_runtime = crate::studio::TaskRuntime::new(store.clone(), product_events);
        let coordinator = TaskCoordinator::new(store.clone(), task_runtime);
        assert!(
            coordinator
                .settle_known_legacy_root_fault(&run)
                .await
                .unwrap()
        );
        assert!(
            coordinator
                .settle_known_legacy_root_fault(&run)
                .await
                .unwrap()
        );

        let settled = store.read_task_run(&run.id).await.unwrap().unwrap();
        assert!(settled.kind().is_terminal());
        assert_eq!(store.list_task_issues(&run.id).await.unwrap().len(), 1);
    }
}

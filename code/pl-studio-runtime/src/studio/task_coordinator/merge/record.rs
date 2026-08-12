use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::Deserialize;

use super::cleanup::cleanup_accepted_delivery;
use super::scope::delivery_from_completion;
use super::validation::{ensure_preflight_delivery_identity, validate_repository_identity};
use crate::studio::task_coordinator::{
    AgentDelivery, BranchLeaseRecord, MergeMethod, MergeRecord, RecordTaskMerge, TaskCoordinator,
    TaskMergeScope, TaskRunPhase, TaskRunRecord, WorkCompletionRecord, WorkUnitRecord,
};
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::{AgentRuntimeHandle, ToolEffect};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskRecordMergeInput {
    /// Executor agent whose accepted completion was integrated.
    pub(crate) executor_agent_id: String,
    /// Accepted completion revision.
    #[schemars(range(min = 1))]
    pub(crate) completion_revision: u32,
    /// Durable Task head before integration.
    pub(crate) expected_previous_head: String,
    /// Task head after integration.
    pub(crate) resulting_head: String,
    /// Git integration method used by the planner.
    pub(crate) method: MergeMethod,
    /// Concise integration summary.
    pub(crate) summary: String,
}

impl TaskCoordinator {
    pub(crate) fn task_record_merge_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<TaskRecordMergeInput>::new(
            "task_record_merge",
            "Validate and record Git integration already performed by the current Task planner.",
        )
        .registered(move |input: TaskRecordMergeInput, context| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            let runtime = runtime.clone();
            async move {
                if context.active_subagent.is_some() {
                    bail!("task_record_merge may only be called by the Task planner");
                }
                let record = coordinator
                    .record_planner_merge(&thread_id, input, Some(&runtime))
                    .await?;
                ToolExecutionResult::<serde_json::Value>::json(record).map_err(anyhow::Error::from)
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) async fn record_planner_merge(
        &self,
        thread_id: &str,
        input: TaskRecordMergeInput,
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<MergeRecord> {
        let executor_agent_id = input.executor_agent_id.trim();
        let summary = input.summary.trim();
        if executor_agent_id.is_empty()
            || input.expected_previous_head.trim().is_empty()
            || input.resulting_head.trim().is_empty()
            || summary.is_empty()
        {
            bail!("task_record_merge string fields must not be empty");
        }
        if let Some(runtime) = runtime {
            self.await_closed_agent_projection(runtime, executor_agent_id)
                .await?;
        }

        if let Some(record) = self
            .find_recorded_planner_merge(thread_id, executor_agent_id, input.completion_revision)
            .await?
        {
            validate_recorded_input(&record, &input, summary)?;
            let scope = self.store.read_accepted_merge_scope(&record.id).await?;
            self.validate_accepted_cleanup_replay(&scope).await?;
            return self.cleanup_recorded_merge(scope, runtime).await;
        }

        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let scope = self
            .load_planner_merge_scope(thread_id, executor_agent_id, input.completion_revision)
            .await?;
        self.ensure_process_lease_owned(&scope.run)?;

        let expected_previous_head = crate::studio::task_coordinator::git::resolve_commit_oid(
            &scope.run.workspace_root,
            input.expected_previous_head.trim(),
        )
        .await?;
        let resulting_head = crate::studio::task_coordinator::git::resolve_commit_oid(
            &scope.run.workspace_root,
            input.resulting_head.trim(),
        )
        .await?;
        ensure!(
            expected_previous_head == scope.run.expected_head
                && expected_previous_head == scope.lease.expected_head,
            "expectedPreviousHead does not match the durable Task head"
        );
        ensure!(
            resulting_head != expected_previous_head,
            "resultingHead must advance beyond expectedPreviousHead"
        );

        validate_repository_identity(
            Path::new(&scope.run.workspace_root),
            Path::new(&scope.run.workspace_root),
            Path::new(&scope.run.git_common_dir),
            &scope.run.branch,
            &resulting_head,
            true,
        )
        .await?;
        validate_repository_identity(
            Path::new(&scope.work_unit.worktree_path),
            Path::new(&scope.work_unit.worktree_path),
            Path::new(&scope.run.git_common_dir),
            &scope.work_unit.branch,
            &scope.delivery.head_commit,
            true,
        )
        .await?;
        crate::studio::task_coordinator::git::ensure_no_git_operation(Path::new(
            &scope.run.workspace_root,
        ))
        .await?;
        ensure!(
            crate::studio::task_coordinator::git::is_ancestor(
                &scope.run.workspace_root,
                &expected_previous_head,
                &resulting_head,
            )
            .await?,
            "expectedPreviousHead must be an ancestor of resultingHead"
        );
        let record = self
            .store
            .record_task_merge(RecordTaskMerge {
                thread_id: thread_id.to_string(),
                executor_agent_id: executor_agent_id.to_string(),
                work_unit_id: scope.work_unit.id.clone(),
                completion_id: scope.completion.id.clone(),
                completion_revision: scope.completion.revision,
                expected_previous_head,
                resulting_head,
                method: input.method,
                summary: summary.to_string(),
            })
            .await?;
        drop(guard);

        let scope = self.store.read_accepted_merge_scope(&record.id).await?;
        self.cleanup_recorded_merge(scope, runtime).await
    }

    async fn load_planner_merge_scope(
        &self,
        thread_id: &str,
        executor_agent_id: &str,
        completion_revision: u32,
    ) -> Result<PlannerMergeScope> {
        let run = self
            .store
            .read_active_task_run_for_root_thread(thread_id)
            .await?;
        if run.phase != TaskRunPhase::Merging {
            bail!("task_record_merge requires phase merging");
        }
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("task branch lease not found")?;
        let work_units = self
            .store
            .list_work_units(&run.id)
            .await?
            .into_iter()
            .filter(|unit| unit.executor_thread_id.as_deref() == Some(executor_agent_id))
            .collect::<Vec<_>>();
        let work_unit = match work_units.as_slice() {
            [unit] => unit.clone(),
            [] => bail!("approved executor work unit not found"),
            _ => bail!("executor owns multiple work units"),
        };
        let completions = self.store.list_work_completions(&run.id).await?;
        let completion = completions
            .into_iter()
            .find(|completion| {
                completion.work_unit_id == work_unit.id
                    && completion.revision == completion_revision
            })
            .context("approved completion revision not found")?;
        let delivery = delivery_from_completion(&completion)?;
        ensure_preflight_delivery_identity(
            &run.id,
            executor_agent_id,
            &work_unit,
            &completion,
            &delivery,
        )?;
        let executor = self
            .store
            .read_thread(executor_agent_id)
            .await?
            .context("executor canonical Thread not found")?;
        ensure!(
            executor.role == "executor" && executor.status == "closed",
            "executor must be canonically closed before merge accounting"
        );
        Ok(PlannerMergeScope {
            run,
            lease,
            work_unit,
            completion,
            delivery,
        })
    }

    async fn find_recorded_planner_merge(
        &self,
        thread_id: &str,
        executor_agent_id: &str,
        completion_revision: u32,
    ) -> Result<Option<MergeRecord>> {
        let run = self
            .store
            .read_active_task_run_for_root_thread(thread_id)
            .await?;
        Ok(self
            .store
            .list_merge_records(&run.id)
            .await?
            .into_iter()
            .find(|record| {
                record.executor_agent_id == executor_agent_id
                    && record.completion_revision == completion_revision
            }))
    }

    async fn cleanup_recorded_merge(
        &self,
        scope: TaskMergeScope,
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<MergeRecord> {
        if matches!(
            scope.merge.cleanup.status.as_str(),
            "discarded" | "alreadyAbsent"
        ) {
            return Ok(scope.merge);
        }
        self.store
            .record_merge_cleanup_attempting(&scope.merge.id)
            .await?;
        let cleanup = cleanup_accepted_delivery(&scope, runtime).await;
        self.store
            .record_merge_cleanup(&scope.merge.id, cleanup)
            .await
    }
}

fn validate_recorded_input(
    record: &MergeRecord,
    input: &TaskRecordMergeInput,
    summary: &str,
) -> Result<()> {
    ensure!(
        record.expected_previous_head == input.expected_previous_head.trim()
            && record.resulting_head == input.resulting_head.trim()
            && record.method == input.method
            && record.summary == summary,
        "task_record_merge retry does not match the recorded merge"
    );
    Ok(())
}

struct PlannerMergeScope {
    run: TaskRunRecord,
    lease: BranchLeaseRecord,
    work_unit: WorkUnitRecord,
    completion: WorkCompletionRecord,
    delivery: AgentDelivery,
}

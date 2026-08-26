use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::Deserialize;

use super::cleanup::cleanup_accepted_delivery;
use super::scope::delivery_from_completion;
use super::validation::ensure_preflight_delivery_identity;
use crate::studio::task_coordinator::{
    MergeMethod, MergeRecord, RecordTaskMerge, TaskCoordinator, TaskMergeScope, TaskRunStateKind,
    WorkCompletionRecord, WorkUnit,
};
use crate::tool::{LocalTool, ToolResult, TypedTool};
use crate::{AgentRuntimeHandle, ToolEffect};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskRecordMergeInput {
    /// Executor agent whose accepted completion was integrated.
    pub(crate) executor_agent_id: String,
    /// Accepted completion revision.
    #[schemars(range(min = 1))]
    pub(crate) completion_revision: u32,
    /// Caller-declared durable ledger value before integration.
    pub(crate) expected_previous_head: String,
    /// Caller-declared durable ledger value after integration.
    pub(crate) resulting_head: String,
    /// Integration method declared by the planner.
    pub(crate) method: MergeMethod,
    /// Concise integration summary.
    pub(crate) summary: String,
}

impl TaskCoordinator {
    pub(crate) fn task_record_merge_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> LocalTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        TypedTool::<TaskRecordMergeInput>::new(
            "task_record_merge",
            "Record integration facts declared by the current Task planner.",
        )
        .handler(move |input: TaskRecordMergeInput, context| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            let runtime = runtime.clone();
            async move {
                if context.identity().parent_agent_id.is_some() {
                    bail!("task_record_merge may only be called by the Task planner");
                }
                let record = coordinator
                    .record_planner_merge(&thread_id, input, Some(&runtime))
                    .await?;
                ToolResult::json(record).map_err(anyhow::Error::from)
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
            let scope = self
                .task_runtime
                .read_accepted_merge_scope(&record.id)
                .await?;
            self.validate_accepted_cleanup_replay(&scope).await?;
            return self.cleanup_recorded_merge(scope, runtime).await;
        }

        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let scope = self
            .load_planner_merge_scope(thread_id, executor_agent_id, input.completion_revision)
            .await?;
        let expected_previous_head = input.expected_previous_head.trim().to_string();
        let resulting_head = input.resulting_head.trim().to_string();
        ensure!(
            resulting_head != expected_previous_head,
            "resultingHead must advance beyond expectedPreviousHead"
        );

        let record = self
            .task_runtime
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

        let scope = self
            .task_runtime
            .read_accepted_merge_scope(&record.id)
            .await?;
        self.cleanup_recorded_merge(scope, runtime).await
    }

    async fn load_planner_merge_scope(
        &self,
        thread_id: &str,
        executor_agent_id: &str,
        completion_revision: u32,
    ) -> Result<PlannerMergeScope> {
        let aggregate = self
            .task_runtime
            .aggregate(thread_id)
            .await
            .context("active Task aggregate is not resident")?;
        let run = aggregate.facts.run;
        if run.kind() != TaskRunStateKind::Working {
            bail!("task_record_merge requires phase merging");
        }
        let work_units = aggregate
            .facts
            .work_units
            .into_iter()
            .filter(|unit| unit.executor_thread_id.as_deref() == Some(executor_agent_id))
            .collect::<Vec<_>>();
        let work_unit = match work_units.as_slice() {
            [unit] => unit.clone(),
            [] => bail!("approved executor work unit not found"),
            _ => bail!("executor owns multiple work units"),
        };
        let completions = aggregate.facts.completions;
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
            executor.role == "executor" && executor.status == pl_protocol::ThreadStatus::Closed,
            "executor must be canonically closed before merge accounting"
        );
        Ok(PlannerMergeScope {
            work_unit,
            completion,
        })
    }

    async fn find_recorded_planner_merge(
        &self,
        thread_id: &str,
        executor_agent_id: &str,
        completion_revision: u32,
    ) -> Result<Option<MergeRecord>> {
        let aggregate = self
            .task_runtime
            .aggregate(thread_id)
            .await
            .context("active Task aggregate is not resident")?;
        Ok(aggregate.facts.merges.into_iter().find(|record| {
            record.executor_agent_id == executor_agent_id
                && record.completion_revision == completion_revision
        }))
    }

    async fn cleanup_recorded_merge(
        &self,
        scope: TaskMergeScope,
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<MergeRecord> {
        if scope.merge.cleanup.is_complete() {
            return Ok(scope.merge);
        }
        let attempt = self
            .task_runtime
            .record_merge_cleanup_attempting(&scope.merge.id)
            .await?;
        let operation_id = attempt
            .cleanup
            .operation_id()
            .context("merge cleanup attempt has no operation id")?
            .to_string();
        let cleanup = cleanup_accepted_delivery(&scope, runtime).await;
        self.task_runtime
            .record_merge_cleanup(&scope.merge.id, &operation_id, cleanup)
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
    work_unit: WorkUnit,
    completion: WorkCompletionRecord,
}

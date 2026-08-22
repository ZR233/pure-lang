use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures::FutureExt;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{DesignFinalizeOutput, TaskCoordinator, TaskRunStateKind};
use crate::ToolEffect;
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskFinalizeDesignInput {
    /// Non-empty summary of the design-stage conclusions.
    summary: String,
}

impl TaskCoordinator {
    /// Design tools no longer observe or fingerprint project files.
    pub(crate) fn design_tool_completion_callback(
        self: &Arc<Self>,
        _task_run_id: String,
        _turn_id: String,
        _workspace: std::path::PathBuf,
    ) -> pl_core::ToolCompletionCallback {
        Arc::new(|_| async { Ok(()) }.boxed())
    }

    pub(crate) fn task_finalize_design_tool(
        self: &Arc<Self>,
        root_thread_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let root_thread_id = root_thread_id.into();
        FunctionToolDefinition::<TaskFinalizeDesignInput>::new(
            "task_finalize_design",
            "Record the design summary and enter implementing. The runtime does not inspect or modify project Git state.",
        )
        .registered(move |arguments: TaskFinalizeDesignInput, context| {
            let coordinator = coordinator.clone();
            let root_thread_id = root_thread_id.clone();
            async move {
                let output = coordinator
                    .finalize_design(
                        &root_thread_id,
                        context.workspace.root(),
                        &arguments.summary,
                    )
                    .await
                    .map_err(|error| anyhow::anyhow!("task_finalize_design failed: {error:#}"))?;
                ToolExecutionResult::<serde_json::Value>::json(output)
                    .map_err(anyhow::Error::from)
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) async fn finalize_design(
        &self,
        root_thread_id: &str,
        _caller_workspace: &std::path::Path,
        summary: &str,
    ) -> Result<DesignFinalizeOutput> {
        let summary = summary.trim();
        if summary.is_empty() {
            bail!("task_finalize_design summary must not be empty");
        }

        let _mutation_guard = self.lock_branch_mutation().await;
        let run = self
            .store
            .read_active_task_run_for_root_thread(root_thread_id)
            .await?;
        if run.kind() != TaskRunStateKind::DesignUpdating {
            bail!(
                "task_finalize_design requires phase designUpdating; current phase is {}",
                run.kind().as_str()
            );
        }
        self.ensure_process_lease_owned(&run)?;
        let lease = self
            .store
            .read_project_lease(&run.id)
            .await?
            .context("task project lease not found")?;
        if lease.task_run_id != run.id || lease.project_id != run.project_id {
            bail!("TaskRun and project lease no longer have the same owner");
        }

        let updated = self
            .store
            .finalize_task_design(&run.id, summary)
            .await?
            .context("task state changed while finalizing the design stage")?;
        Ok(DesignFinalizeOutput {
            task_run_id: updated.id.clone(),
            summary: summary.to_string(),
            revision: updated.revision,
            state: updated.kind(),
        })
    }
}

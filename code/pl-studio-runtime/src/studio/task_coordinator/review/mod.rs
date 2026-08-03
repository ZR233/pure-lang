mod exit;
pub(crate) mod prompt;
mod trace;

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::design::design_commit_is_current;
use super::{
    AgentOutcomeRecord, MergeRecord, ReviewRoundRecord, ReviewScope, StudioSpawnIntent,
    TaskCoordinator, TaskRunPhase, TaskRunRecord, WorkCompletionRecord, WorkUnitRecord,
};
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::{
    AgentRoleId, AgentRuntimeHandle, AgentSessionState, AgentSpawnRequest, SessionId, ToolEffect,
};

const REVIEWER_CONSTRAINT: &str = "你是只读代码审查者。审查目标由 prompt 中的 scope、精确 completion revision 或 Task HEAD 唯一绑定。先检查 plan、目标 diff、ownedPaths、验证摘要和受影响代码。delivery scope 的 verdict 与 findings 只能针对当前 completion diff 和目标 WorkUnit ownedPaths；其他 WorkUnit 仅是延后集成的 ownership 上下文，不得把尚未合并的 sibling 文件、跨 WorkUnit 交互或任务整体完整性归责给当前 executor，这些内容由 integrated review 审查。在读取 design 正文前必须先调用 search_files 或 list_files 定位文档，再用 read_file 阅读。design 正文必须以 path=design/... 且省略 cwd 或 cwd=. 的 workspace-root 相对形式读取；completion worktree 的 cwd 只用于读取目标 source。最终必须成功调用 review_exit；pass 的 findings 必须为空，changesRequired/blocked 必须提供具体 finding。只能调用只读工具与 review_exit；禁止修改、派生代理、修复、合并或宣布 Task 完成。";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestDeliveryReviewInput {
    executor_agent_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestIntegratedReviewInput {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskStatusInput {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestReviewOutput {
    review_round_id: String,
    reviewer_agent_id: String,
    scope: ReviewScope,
    reviewed_head: String,
    completion_id: Option<String>,
    completion_revision: Option<u32>,
    round: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskStatusOutput {
    run: TaskRunRecord,
    work_units: Vec<WorkUnitRecord>,
    completions: Vec<WorkCompletionRecord>,
    outcomes: Vec<AgentOutcomeRecord>,
    merges: Vec<MergeRecord>,
    reviews: Vec<ReviewRoundRecord>,
}

impl TaskCoordinator {
    pub(crate) fn task_request_delivery_review_tool(
        self: &Arc<Self>,
        session_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let session_id = session_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_request_delivery_review",
            "Start one fresh read-only reviewer for the latest completion of an executor.",
            strict_tool_input_schema([ToolInputSchemaField::required(
                "executorAgentId",
                serde_json::json!({"type": "string"}),
            )]),
            move |input: RequestDeliveryReviewInput, context| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                let runtime = runtime.clone();
                async move {
                    let call_id = provider_call_id(
                        context.provider_call_id.as_deref(),
                        "task_request_delivery_review",
                    )?;
                    let round = coordinator
                        .store
                        .begin_delivery_review(
                            &session_id,
                            input.executor_agent_id.trim(),
                            &call_id,
                        )
                        .await?;
                    coordinator
                        .spawn_reviewer(
                            &session_id,
                            round,
                            &call_id,
                            &runtime,
                            context.workspace_root,
                        )
                        .await
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) fn task_request_integrated_review_tool(
        self: &Arc<Self>,
        session_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let session_id = session_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_request_integrated_review",
            "Start one fresh read-only integrated review for the current Task HEAD.",
            strict_tool_input_schema([]),
            move |_: RequestIntegratedReviewInput, context| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                let runtime = runtime.clone();
                async move {
                    let call_id = provider_call_id(
                        context.provider_call_id.as_deref(),
                        "task_request_integrated_review",
                    )?;
                    let guard = coordinator.lock_branch_mutation().await;
                    let run = coordinator
                        .preflight_integrated_review_locked(&session_id, &guard)
                        .await?;
                    let round = coordinator
                        .store
                        .begin_integrated_review(&session_id, &call_id)
                        .await?;
                    drop(guard);
                    debug_assert_eq!(round.reviewed_head, run.expected_head);
                    coordinator
                        .spawn_reviewer(
                            &session_id,
                            round,
                            &call_id,
                            &runtime,
                            context.workspace_root,
                        )
                        .await
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) fn task_status_tool(
        self: &Arc<Self>,
        session_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let session_id = session_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_status",
            "Read the canonical durable Task state, completions, reviews, merges, and findings.",
            strict_tool_input_schema([]),
            move |_: TaskStatusInput, _| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                async move {
                    let run = coordinator
                        .store
                        .read_active_task_run_for_session(&session_id)
                        .await?;
                    let output = TaskStatusOutput {
                        work_units: coordinator.store.list_work_units(&run.id).await?,
                        completions: coordinator.store.list_work_completions(&run.id).await?,
                        outcomes: coordinator.store.list_agent_outcomes(&run.id).await?,
                        merges: coordinator.store.list_merge_records(&run.id).await?,
                        reviews: coordinator.store.list_review_rounds(&run.id).await?,
                        run,
                    };
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::Read)
    }

    async fn spawn_reviewer(
        self: &Arc<Self>,
        session_id: &str,
        round: ReviewRoundRecord,
        call_id: &str,
        runtime: &AgentRuntimeHandle,
        workspace_root: std::path::PathBuf,
    ) -> Result<ToolExecutionResult> {
        let prompt = match prompt::build_review_prompt(self, &round).await {
            Ok(prompt) => prompt,
            Err(error) => {
                self.store
                    .fail_reviewer_spawn(session_id, None, call_id, &error.to_string())
                    .await?;
                return Err(error);
            }
        };
        let reviewer_session_id = SessionId::generate();
        let intent = StudioSpawnIntent::task_reviewer(
            session_id,
            format!("{}_review_round_{}", round.scope.as_str(), round.round),
            call_id,
            workspace_root,
            REVIEWER_CONSTRAINT,
        );
        let spawn = runtime
            .spawn(AgentSpawnRequest {
                parent_id: crate::studio::agent_host::root_agent_id(session_id),
                role: AgentRoleId::new("reviewer")
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                session: AgentSessionState::empty(reviewer_session_id),
                initial_message: Some(prompt),
                metadata: serde_json::to_value(intent)?,
            })
            .await;
        let handle = match spawn {
            Ok(handle) => handle,
            Err(error) => {
                self.store
                    .fail_reviewer_spawn(session_id, None, call_id, &error.to_string())
                    .await?;
                return Err(anyhow::anyhow!(error.to_string()));
            }
        };
        ToolExecutionResult::<serde_json::Value>::json(RequestReviewOutput {
            review_round_id: round.id,
            reviewer_agent_id: handle.snapshot.identity.id.to_string(),
            scope: round.scope,
            reviewed_head: round.reviewed_head,
            completion_id: round.completion_id,
            completion_revision: round.completion_revision,
            round: round.round,
        })
        .map_err(anyhow::Error::from)
    }

    pub(super) async fn preflight_integrated_review_locked(
        &self,
        session_id: &str,
        guard: &super::BranchMutationGuard<'_>,
    ) -> Result<TaskRunRecord> {
        self.ensure_branch_mutation_guard(guard)?;
        let run = self
            .store
            .read_active_task_run_for_session(session_id)
            .await?;
        if !matches!(
            run.phase,
            TaskRunPhase::Implementing | TaskRunPhase::Reworking
        ) {
            bail!("integrated review requires implementing or reworking");
        }
        self.ensure_process_lease_owned(&run)?;
        validate_review_repository(&run).await?;
        if !design_commit_is_current(&run) {
            bail!("integrated review requires final task_update_design for the current HEAD");
        }
        Ok(run)
    }
}

fn provider_call_id(value: Option<&str>, tool: &str) -> Result<String> {
    let value = value
        .context(format!("{tool} requires a provider call id"))?
        .trim();
    if value.is_empty() {
        bail!("{tool} requires a non-empty provider call id");
    }
    Ok(value.to_string())
}

pub(super) async fn validate_review_repository(run: &TaskRunRecord) -> Result<()> {
    let snapshot = super::git::inspect_repository(&run.workspace_root, true).await?;
    let common = std::fs::canonicalize(&snapshot.git_common_dir)
        .unwrap_or(snapshot.git_common_dir)
        .to_string_lossy()
        .replace('\\', "/");
    let expected_common = std::fs::canonicalize(&run.git_common_dir)
        .unwrap_or_else(|_| std::path::PathBuf::from(&run.git_common_dir))
        .to_string_lossy()
        .replace('\\', "/");
    let equal_common = if cfg!(windows) {
        common.eq_ignore_ascii_case(&expected_common)
    } else {
        common == expected_common
    };
    if !equal_common || snapshot.branch != run.branch || snapshot.head != run.expected_head {
        bail!("task branch identity or HEAD drifted before review");
    }
    Ok(())
}

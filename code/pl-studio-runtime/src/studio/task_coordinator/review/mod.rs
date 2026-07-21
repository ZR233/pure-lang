mod exit;
pub(crate) mod prompt;
mod trace;

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{TaskCoordinator, TaskRunPhase};
use crate::tool::{RegisteredTool, ToolExecutionResult, strict_tool_input_schema};
use crate::{
    AgentRoleId, AgentRuntimeHandle, AgentSessionForkPolicy, AgentSessionState, AgentSpawnRequest,
    SessionId, ToolEffect,
};

const REVIEWER_CONSTRAINT: &str = "你是只读代码审查者。先检查完整 plan、综合 diff 和受影响代码。工具顺序是强制门禁：在任何 design read_file 之前，必须先调用 search_files 或 list_files 定位相关 design 文档；然后用 read_file 阅读审查所需正文。对照实际读取的 design 检查一致性，同时检查 bug、回归、安全与测试缺口。所有设计结论必须引用实际读取的 design 路径和章节。最终必须成功调用 review_exit，普通文本结论不算完成；如果结论为 pass，findings 必须是空数组，不要把已通过的检查或 info 说明作为 finding；如果 review_exit 被拒绝，必须根据错误补齐 locator、重新 read_file 并再次调用 review_exit。只能调用只读工具与 review_exit；禁止修改、派生代理、修复、合并或宣布任务完成。";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestReviewInput {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestReviewOutput {
    review_round_id: String,
    reviewer_agent_id: String,
    head_commit: String,
    round: u32,
}

impl TaskCoordinator {
    pub(crate) fn task_request_review_tool(
        self: &Arc<Self>,
        session_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let session_id = session_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_request_review",
            "Start one harness-owned read-only review for the current task HEAD.",
            strict_tool_input_schema([]),
            move |_: RequestReviewInput, context| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                let runtime = runtime.clone();
                async move {
                    let call_id = context
                        .provider_call_id
                        .as_deref()
                        .context("task_request_review requires a provider call id")?
                        .to_string();
                    let guard = coordinator.lock_branch_mutation().await;
                    coordinator.ensure_branch_mutation_guard(&guard)?;
                    let run = coordinator
                        .store
                        .read_active_task_run_for_session(&session_id)
                        .await?;
                    if !matches!(
                        run.phase,
                        TaskRunPhase::Implementing | TaskRunPhase::Reworking
                    ) {
                        bail!("task_request_review requires implementing or reworking");
                    }
                    coordinator.ensure_process_lease_owned(&run)?;
                    validate_review_repository(&run).await?;
                    let prompt = prompt::build_review_prompt(&coordinator, &run).await?;
                    let round = coordinator
                        .store
                        .begin_task_review(&session_id, &call_id)
                        .await?;
                    drop(guard);
                    let reviewer_session_id = SessionId::generate();
                    let spawn = runtime
                        .spawn(AgentSpawnRequest {
                            parent_id: crate::studio::agent_host::root_agent_id(&session_id),
                            role: AgentRoleId::new("reviewer")
                                .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                            session: AgentSessionState {
                                id: reviewer_session_id,
                                metadata: serde_json::json!({
                                    "subagentConstraint": REVIEWER_CONSTRAINT,
                                }),
                                session: context
                                    .parent_session
                                    .fork(AgentSessionForkPolicy::AllMessages),
                                usage: pl_model::TokenUsage::default(),
                                last_context_tokens: None,
                                trace_sequence: 0,
                                session_event_sequence: 0,
                            },
                            initial_message: Some(prompt),
                            metadata: serde_json::json!({
                                "studioSessionId": session_id,
                                "taskName": format!("review_round_{}", round.round),
                                "ownedPaths": [],
                                "requestingToolCallId": call_id,
                                "workspaceRoot": context.workspace_root,
                                "subagentConstraint": REVIEWER_CONSTRAINT,
                            }),
                        })
                        .await;
                    let handle = match spawn {
                        Ok(handle) => handle,
                        Err(error) => {
                            let _ = coordinator
                                .store
                                .fail_reviewer_spawn(
                                    &session_id,
                                    None,
                                    &call_id,
                                    &error.to_string(),
                                )
                                .await;
                            return Err(anyhow::anyhow!(error.to_string()));
                        }
                    };
                    ToolExecutionResult::<serde_json::Value>::json(RequestReviewOutput {
                        review_round_id: round.id,
                        reviewer_agent_id: handle.snapshot.identity.id.to_string(),
                        head_commit: round.head_commit,
                        round: round.round,
                    })
                    .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }
}

pub(super) async fn validate_review_repository(run: &super::TaskRunRecord) -> Result<()> {
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

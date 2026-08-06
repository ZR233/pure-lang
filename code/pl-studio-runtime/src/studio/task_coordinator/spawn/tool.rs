use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{StudioSpawnIntent, normalize_scope_hints};
use crate::studio::task_coordinator::TaskCoordinator;
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::{
    AgentRoleId, AgentRuntimeHandle, AgentSpawnRequest, ThreadContextState, ThreadId, ToolEffect,
};

const MAX_EXECUTOR_CONSTRAINT_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskSpawnExecutorInput {
    task_name: String,
    message: String,
    #[serde(default)]
    scope_hints: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskSpawnExecutorOutput {
    agent_id: String,
    thread_id: String,
    turn_id: String,
    scope_hints: Vec<String>,
}

impl TaskCoordinator {
    pub(crate) fn task_spawn_executor_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_spawn_executor",
            "Spawn one Task executor with a fresh session and optional repository-relative scope hints.",
            strict_tool_input_schema([
                ToolInputSchemaField::required(
                    "taskName",
                    serde_json::json!({ "type": "string", "minLength": 1 }),
                ),
                ToolInputSchemaField::required(
                    "message",
                    serde_json::json!({ "type": "string", "minLength": 1 }),
                ),
                ToolInputSchemaField::optional(
                    "scopeHints",
                    serde_json::json!({
                        "type": "array",
                        "description": "Optional repository-relative path prefixes used only for task decomposition, review focus, and potential-conflict hints. They do not restrict workspace writes; directories do not require `/**`.",
                        "items": {
                            "type": "string",
                            "minLength": 1,
                            "description": "A normalized repository-relative path prefix."
                        }
                    }),
                ),
            ]),
            move |arguments: TaskSpawnExecutorInput, context| {
                let runtime = runtime.clone();
                let thread_id = thread_id.clone();
                async move {
                    let task_name = arguments.task_name.trim();
                    if task_name.is_empty() {
                        bail!("taskName must not be empty");
                    }
                    if arguments.message.trim().is_empty() {
                        bail!("message must not be empty");
                    }
                    let scope_hints = normalize_scope_hints(&arguments.scope_hints)?;
                    let constraint = executor_constraint(&scope_hints)?;
                    let call_id = context
                        .provider_call_id
                        .as_deref()
                        .context("task_spawn_executor requires a provider call id")?
                        .to_string();
                    let child_thread_id = ThreadId::generate();
                    let intent = StudioSpawnIntent::task_executor(
                        &thread_id,
                        task_name,
                        scope_hints.clone(),
                        call_id,
                        constraint,
                    );
                    let result = runtime
                        .spawn(AgentSpawnRequest {
                            thread_id: child_thread_id.clone(),
                            parent_id: crate::studio::agent_host::root_agent_id(&thread_id),
                            role: AgentRoleId::new("executor")
                                .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                            session: ThreadContextState::empty(),
                            initial_message: Some(arguments.message),
                            metadata: serde_json::to_value(intent)?,
                        })
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                    let turn_id = result
                        .initial_turn_id
                        .context("task executor spawn did not create an initial turn")?;
                    ToolExecutionResult::<serde_json::Value>::json(TaskSpawnExecutorOutput {
                        agent_id: result.snapshot.identity.id.to_string(),
                        thread_id: child_thread_id.to_string(),
                        turn_id: turn_id.to_string(),
                        scope_hints,
                    })
                    .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }
}

fn executor_constraint(scope_hints: &[String]) -> Result<String> {
    let paths = scope_hints
        .iter()
        .map(|path| format!("- {path}"))
        .collect::<Vec<_>>()
        .join("\n");
    let paths = if paths.is_empty() {
        "- （未提供；以完整任务说明为准）".to_string()
    } else {
        paths
    };
    let constraint = format!(
        "你是 Task executor，只能在系统分配给你的独立 worktree 中工作。\
\n以下 scopeHints 仅用于理解任务拆分和审查重点，不是文件写入边界；worktree 内的必要修改均允许：\n{paths}\
\n完成定位、开始实现、开始验证、遇到阻塞和准备提交完成报告时，调用 \
report_progress 记录准确摘要与下一步；它不是心跳。准备提交时使用 readyForCompletion，\
该 checkpoint 不表示已完成或可审查。完成后必须自行验证、提交所有变更，并调用 \
report_completion 提交实际 HEAD 与验证摘要；只有该工具成功才产生 readyForReview，普通文本回复不算完成。\
\n工具失败后先读取当前状态，修复根因或换一种方案，不得原样重复同一个失败调用。\
\n不得派生代理、合并分支、切换/创建/删除分支、操作 planner 或用户工作区，\
也不得自行把提交合入任务分支。"
    );
    if constraint.len() > MAX_EXECUTOR_CONSTRAINT_BYTES {
        bail!("scopeHints are too large for executor instructions");
    }
    Ok(constraint)
}

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{
    StudioSpawnIntent, StudioTaskExecutorIntent, TaskExecutorDependencyV1, TaskExecutorEvidenceV1,
    TaskExecutorVerificationCommandV1, normalize_scope_hints,
};
use crate::studio::task_coordinator::{AllocateExecutor, TaskCoordinator};
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::{
    AgentRoleId, AgentRuntimeHandle, AgentSpawnRequest, ThreadContextState, ThreadId, ToolEffect,
    TurnId,
};

const MAX_EXECUTOR_CONSTRAINT_BYTES: usize = 16 * 1024;
const MAX_EXECUTOR_VERIFICATION_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskSpawnExecutorInput {
    task_name: String,
    message: String,
    #[serde(default)]
    scope_hints: Vec<String>,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
    #[serde(default)]
    dependencies: Vec<TaskExecutorDependencyV1>,
    #[serde(default)]
    evidence: Vec<TaskExecutorEvidenceV1>,
    verification_commands: Vec<TaskExecutorVerificationCommandV1>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskSpawnExecutorOutput {
    agent_id: String,
    thread_id: String,
    turn_id: String,
    scope_hints: Vec<String>,
    reused: bool,
}

impl TaskCoordinator {
    pub(crate) fn task_spawn_executor_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let thread_id = thread_id.into();
        let coordinator = Arc::clone(self);
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
                ToolInputSchemaField::optional(
                    "acceptanceCriteria",
                    serde_json::json!({
                        "type": "array",
                        "items": { "type": "string", "minLength": 1 }
                    }),
                ),
                ToolInputSchemaField::optional(
                    "dependencies",
                    serde_json::json!({
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["kind", "id"],
                            "properties": {
                                "kind": { "type": "string", "minLength": 1 },
                                "id": { "type": "string", "minLength": 1 },
                                "note": { "type": "string" }
                            }
                        }
                    }),
                ),
                ToolInputSchemaField::optional(
                    "evidence",
                    serde_json::json!({
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["path"],
                            "properties": {
                                "path": { "type": "string", "minLength": 1 },
                                "line": { "type": "integer", "minimum": 1 },
                                "symbol": { "type": "string" },
                                "contentHash": { "type": "string" },
                                "note": { "type": "string" }
                            }
                        }
                    }),
                ),
                ToolInputSchemaField::required(
                    "verificationCommands",
                    serde_json::json!({
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["command", "cwd", "purpose"],
                            "properties": {
                                "command": { "type": "string", "minLength": 1 },
                                "cwd": {
                                    "type": "string",
                                    "minLength": 1,
                                    "description": "Repository-relative working directory, or `.` for the repository root."
                                },
                                "purpose": { "type": "string", "minLength": 1 }
                            }
                        }
                    }),
                ),
            ]),
            move |arguments: TaskSpawnExecutorInput, context| {
                let runtime = runtime.clone();
                let thread_id = thread_id.clone();
                let coordinator = Arc::clone(&coordinator);
                async move {
                    let task_name = arguments.task_name.trim();
                    if task_name.is_empty() {
                        bail!("taskName must not be empty");
                    }
                    if arguments.message.trim().is_empty() {
                        bail!("message must not be empty");
                    }
                    let scope_hints = normalize_scope_hints(&arguments.scope_hints)?;
                    let acceptance_criteria = normalize_non_empty(arguments.acceptance_criteria)?;
                    let dependencies = normalize_dependencies(arguments.dependencies)?;
                    let evidence = normalize_evidence(arguments.evidence)?;
                    let verification_commands =
                        normalize_verification_commands(arguments.verification_commands)?;
                    let constraint = executor_constraint(&scope_hints)?;
                    let call_id = context
                        .provider_call_id
                        .as_deref()
                        .context("task_spawn_executor requires a provider call id")?
                        .to_string();
                    let (requested_thread_id, _) =
                        executor_runtime_ids(&thread_id, &call_id)?;
                    let allocation = coordinator
                        .reserve_executor_spawn(AllocateExecutor {
                            thread_id: thread_id.clone(),
                            title: task_name.to_string(),
                            scope_hints: scope_hints.clone(),
                            agent_id: requested_thread_id.to_string(),
                            requested_by_call_id: call_id.clone(),
                        })
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                    let canonical_call_id = allocation.work_unit.requested_by_call_id.clone();
                    let (child_thread_id, initial_turn_id) =
                        executor_runtime_ids(&thread_id, &canonical_call_id)?;
                    if allocation.work_unit.executor_thread_id.as_deref()
                        != Some(child_thread_id.as_str())
                    {
                        bail!(
                            "durable executor identity does not match its canonical allocation"
                        );
                    }
                    if runtime.snapshot(child_thread_id.clone()).await.is_ok() {
                        return ToolExecutionResult::<serde_json::Value>::json(
                            TaskSpawnExecutorOutput {
                                agent_id: child_thread_id.to_string(),
                                thread_id: child_thread_id.to_string(),
                                turn_id: initial_turn_id.to_string(),
                                scope_hints,
                                reused: true,
                            },
                        )
                        .map_err(anyhow::Error::from);
                    }
                    let assignment = arguments.message;
                    let intent = StudioSpawnIntent::task_executor(StudioTaskExecutorIntent {
                        thread_id: thread_id.clone(),
                        task_name: task_name.to_string(),
                        scope_hints: scope_hints.clone(),
                        requesting_tool_call_id: canonical_call_id,
                        subagent_constraint: constraint,
                        assignment: assignment.clone(),
                        acceptance_criteria,
                        dependencies,
                        evidence,
                        verification_commands,
                    });
                    let result = runtime
                        .spawn(AgentSpawnRequest {
                            thread_id: child_thread_id.clone(),
                            parent_id: crate::studio::agent_host::root_agent_id(&thread_id),
                            role: AgentRoleId::new("executor")
                                .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                            session: ThreadContextState::empty(),
                            initial_turn_id: Some(initial_turn_id),
                            initial_message: Some(assignment),
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
                        reused: allocation.reused,
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
\n不得派生代理、合并分支、切换/创建/删除分支、操作 planner 或用户工作区，\
也不得自行把提交合入任务分支。"
    );
    if constraint.len() > MAX_EXECUTOR_CONSTRAINT_BYTES {
        bail!("scopeHints are too large for executor instructions");
    }
    Ok(constraint)
}

fn executor_runtime_ids(thread_id: &str, call_id: &str) -> Result<(ThreadId, TurnId)> {
    let hash = pl_core::canonical_content_hash(format!("{thread_id}\0{call_id}").as_bytes());
    let digest = hash
        .strip_prefix("sha256:")
        .context("canonical content hash omitted the sha256 prefix")?;
    if digest.len() < 32 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("canonical content hash is not a full hexadecimal digest")
    }
    let thread = ThreadId::new(format!("thread-task-{}", &digest[..16]))
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let turn = TurnId::new(format!("turn-task-{}", &digest[16..32]))
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok((thread, turn))
}

fn normalize_non_empty(values: Vec<String>) -> Result<Vec<String>> {
    values
        .into_iter()
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                bail!("acceptanceCriteria must not contain empty values")
            }
            Ok(value.to_string())
        })
        .collect()
}

fn normalize_dependencies(
    mut dependencies: Vec<TaskExecutorDependencyV1>,
) -> Result<Vec<TaskExecutorDependencyV1>> {
    for dependency in &mut dependencies {
        dependency.kind = dependency.kind.trim().to_string();
        dependency.id = dependency.id.trim().to_string();
        if dependency.kind.is_empty() || dependency.id.is_empty() {
            bail!("executor dependencies require non-empty kind and id")
        }
    }
    Ok(dependencies)
}

fn normalize_evidence(
    mut evidence: Vec<TaskExecutorEvidenceV1>,
) -> Result<Vec<TaskExecutorEvidenceV1>> {
    for item in &mut evidence {
        item.path = normalize_scope_hints(std::slice::from_ref(&item.path))?
            .into_iter()
            .next()
            .context("executor evidence path is missing")?;
    }
    Ok(evidence)
}

fn normalize_verification_commands(
    mut commands: Vec<TaskExecutorVerificationCommandV1>,
) -> Result<Vec<TaskExecutorVerificationCommandV1>> {
    if commands.is_empty() {
        bail!("verificationCommands must not be empty")
    }
    for item in &mut commands {
        item.command = item.command.trim().to_string();
        item.purpose = item.purpose.trim().to_string();
        let cwd = item.cwd.trim();
        item.cwd = if cwd == "." {
            cwd.to_string()
        } else {
            normalize_scope_hints(&[cwd.to_string()])?
                .into_iter()
                .next()
                .context("verification command cwd is missing")?
        };
        if item.command.is_empty() || item.purpose.is_empty() {
            bail!("verificationCommands require non-empty command and purpose")
        }
    }
    if serde_json::to_vec(&commands)?.len() > MAX_EXECUTOR_VERIFICATION_BYTES {
        bail!("verificationCommands are too large for executor handoff")
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_runtime_ids_are_stable_git_ref_components() {
        let first = executor_runtime_ids("thread-root", "call-spawn").unwrap();
        let repeated = executor_runtime_ids("thread-root", "call-spawn").unwrap();
        let different = executor_runtime_ids("thread-root", "call-other").unwrap();

        assert_eq!(first, repeated);
        assert_ne!(first, different);
        for id in [first.0.to_string(), first.1.to_string()] {
            assert!(!id.contains(':'));
            assert!(
                id.rsplit('-')
                    .next()
                    .is_some_and(|digest| digest.len() == 16
                        && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
            );
        }
    }

    #[test]
    fn verification_commands_are_non_empty_and_repository_relative() {
        let normalized = normalize_verification_commands(vec![
            TaskExecutorVerificationCommandV1 {
                command: " cargo test --workspace ".to_string(),
                cwd: ".".to_string(),
                purpose: " run workspace tests ".to_string(),
            },
            TaskExecutorVerificationCommandV1 {
                command: "cargo test".to_string(),
                cwd: "code\\example".to_string(),
                purpose: "run component tests".to_string(),
            },
        ])
        .unwrap();

        assert_eq!(normalized[0].command, "cargo test --workspace");
        assert_eq!(normalized[0].cwd, ".");
        assert_eq!(normalized[1].cwd, "code/example");
        assert!(normalize_verification_commands(Vec::new()).is_err());
        assert!(
            normalize_verification_commands(vec![TaskExecutorVerificationCommandV1 {
                command: "cargo test".to_string(),
                cwd: "../outside".to_string(),
                purpose: "verify".to_string(),
            }],)
            .is_err()
        );
    }
}

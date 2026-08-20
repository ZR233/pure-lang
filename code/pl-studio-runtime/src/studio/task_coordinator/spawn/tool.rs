use std::sync::Arc;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    StudioSpawnIntent, StudioTaskExecutorIntent, TaskExecutorAcceptanceCriterion,
    TaskExecutorBlueprint, TaskExecutorDependency, TaskExecutorEvidence,
    TaskExecutorImplementationStep, TaskExecutorScope, TaskExecutorVerificationContract,
};
use crate::studio::task_coordinator::{AllocateExecutor, TaskCoordinator};
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::{
    AgentRoleId, AgentRuntimeHandle, AgentSpawnRequest, ThreadContextState, ThreadId, ToolEffect,
    TurnId,
};

const MAX_EXECUTOR_CONSTRAINT_BYTES: usize = 16 * 1024;
const EXECUTOR_INITIAL_MESSAGE: &str =
    "读取固定的 Task executor handoff，按实施步骤顺序开始工作；不要依赖 planner 对话历史。";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskSpawnExecutorInput {
    /// Stable task name for this independently verifiable work unit.
    #[schemars(length(min = 1))]
    task_name: String,
    /// Concrete outcome this work unit must deliver.
    #[schemars(length(min = 1))]
    objective: String,
    /// Explicit semantic and repository scope.
    scope: TaskExecutorScope,
    /// Ordered implementation blueprint with repository targets.
    #[schemars(length(min = 1))]
    implementation_steps: Vec<TaskExecutorImplementationStep>,
    /// Stable, referenced acceptance criteria.
    #[schemars(length(min = 1))]
    acceptance_criteria: Vec<TaskExecutorAcceptanceCriterion>,
    /// Structured dependencies known to the planner.
    dependencies: Vec<TaskExecutorDependency>,
    /// Stable repository evidence already collected.
    evidence: Vec<TaskExecutorEvidence>,
    /// Commands and inspections that prove every acceptance criterion.
    verification: TaskExecutorVerificationContract,
}

impl TaskSpawnExecutorInput {
    fn into_blueprint(self) -> Result<TaskExecutorBlueprint> {
        TaskExecutorBlueprint {
            task_name: self.task_name,
            objective: self.objective,
            scope: self.scope,
            implementation_steps: self.implementation_steps,
            acceptance_criteria: self.acceptance_criteria,
            dependencies: self.dependencies,
            evidence: self.evidence,
            verification: self.verification,
        }
        .normalize_and_validate()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskSpawnExecutorOutput {
    agent_id: String,
    thread_id: String,
    turn_id: String,
    scope_hints: Vec<String>,
    blueprint_fingerprint: String,
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
        FunctionToolDefinition::<TaskSpawnExecutorInput>::new(
            "task_spawn_executor",
            "Spawn one Task executor from a concrete, self-contained implementation blueprint.",
        )
        .registered(move |arguments: TaskSpawnExecutorInput, context| {
            let runtime = runtime.clone();
            let thread_id = thread_id.clone();
            let coordinator = Arc::clone(&coordinator);
            async move {
                // Blueprint validation and context budgeting must precede every durable allocation.
                let blueprint = arguments.into_blueprint()?;
                let blueprint_fingerprint = blueprint.fingerprint()?;
                let scope_hints = blueprint.scope.scope_hints.clone();
                let constraint = executor_constraint(&scope_hints)?;
                let call_id = context
                    .provider_call_id
                    .as_deref()
                    .context("task_spawn_executor requires a provider call id")?
                    .to_string();
                let (requested_thread_id, _) = executor_runtime_ids(&thread_id, &call_id)?;
                let allocation = coordinator
                    .reserve_executor_spawn(AllocateExecutor {
                        thread_id: thread_id.clone(),
                        title: blueprint.task_name.clone(),
                        scope_hints: scope_hints.clone(),
                        agent_id: requested_thread_id.to_string(),
                        requested_by_call_id: call_id,
                    })
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                if allocation.reused {
                    let (_, existing) = coordinator
                        .store
                        .read_work_unit_handoff(&allocation.work_unit.id)
                        .await?
                        .context("reused executor allocation has no durable handoff")?;
                    ensure_reused_blueprint_matches(
                        &existing.blueprint_fingerprint,
                        &blueprint_fingerprint,
                    )?;
                }
                let canonical_call_id = allocation.work_unit.requested_by_call_id.clone();
                let (child_thread_id, initial_turn_id) =
                    executor_runtime_ids(&thread_id, &canonical_call_id)?;
                if allocation.work_unit.executor_thread_id.as_deref()
                    != Some(child_thread_id.as_str())
                {
                    bail!("durable executor identity does not match its canonical allocation");
                }
                if runtime.snapshot(child_thread_id.clone()).await.is_ok() {
                    return spawn_output(
                        child_thread_id,
                        initial_turn_id,
                        scope_hints,
                        blueprint_fingerprint,
                        true,
                    );
                }
                let intent = StudioSpawnIntent::task_executor(StudioTaskExecutorIntent {
                    thread_id: thread_id.clone(),
                    requesting_tool_call_id: canonical_call_id,
                    subagent_constraint: constraint,
                    blueprint,
                });
                let result = runtime
                    .spawn(AgentSpawnRequest {
                        thread_id: child_thread_id.clone(),
                        parent_id: crate::studio::agent_host::root_agent_id(&thread_id),
                        role: AgentRoleId::new("executor")
                            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                        session: ThreadContextState::empty(),
                        initial_turn_id: Some(initial_turn_id),
                        initial_message: Some(EXECUTOR_INITIAL_MESSAGE.to_string()),
                        metadata: serde_json::to_value(intent)?,
                    })
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                let turn_id = result
                    .initial_turn_id
                    .context("task executor spawn did not create an initial turn")?;
                spawn_output(
                    child_thread_id,
                    turn_id,
                    scope_hints,
                    blueprint_fingerprint,
                    allocation.reused,
                )
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }
}

fn ensure_reused_blueprint_matches(existing: &str, requested: &str) -> Result<()> {
    if existing != requested {
        bail!("executor allocation conflicts with the existing implementation blueprint")
    }
    Ok(())
}

fn spawn_output(
    child_thread_id: ThreadId,
    turn_id: TurnId,
    scope_hints: Vec<String>,
    blueprint_fingerprint: String,
    reused: bool,
) -> Result<ToolExecutionResult<serde_json::Value>> {
    ToolExecutionResult::<serde_json::Value>::json(TaskSpawnExecutorOutput {
        agent_id: child_thread_id.to_string(),
        thread_id: child_thread_id.to_string(),
        turn_id: turn_id.to_string(),
        scope_hints,
        blueprint_fingerprint,
        reused,
    })
    .map_err(anyhow::Error::from)
}

fn executor_constraint(scope_hints: &[String]) -> Result<String> {
    let paths = scope_hints
        .iter()
        .map(|path| format!("- {path}"))
        .collect::<Vec<_>>()
        .join("\n");
    let constraint = format!(
        "你是 Task executor，只能在系统分配给你的独立 worktree 中工作。\
\n先读取 studio.task_executor_handoff；它是目标、范围、步骤、验收和验证的唯一契约，\
不要依赖 planner 对话历史。按步骤顺序工作，可调整不改变任务语义的低层实现细节。\
若仓库事实与蓝图冲突，或必须扩大目标、范围或验收语义，保留证据并通知 planner。\
\n以下 scopeHints 用于拆分、审查和冲突提示，不是文件写入边界：\n{paths}\
\n开始实现、开始验证、遇到阻塞和准备提交完成报告时，用 report_progress 记录准确摘要。\
完成后必须执行 handoff 的全部验证、提交所有变更，并调用 report_completion；\
verificationResults 必须按 checkId 恰好覆盖全部命令和检查。普通文本回复不算完成。\
\n不得派生代理、合并分支、切换/创建/删除分支、操作 planner 或用户工作区，\
也不得自行把提交合入任务分支。"
    );
    if constraint.len() > MAX_EXECUTOR_CONSTRAINT_BYTES {
        bail!("scope.scopeHints are too large for executor instructions");
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
    fn vague_legacy_executor_assignment_is_rejected_before_allocation() {
        let legacy = serde_json::json!({
            "taskName": "do it",
            "message": "fix the code",
            "scopeHints": [],
            "verificationCommands": [{
                "command": "cargo test",
                "cwd": ".",
                "purpose": "test"
            }]
        });
        assert!(serde_json::from_value::<TaskSpawnExecutorInput>(legacy).is_err());
    }

    #[test]
    fn structured_executor_assignment_rejects_unknown_legacy_fields() {
        let mut input = serde_json::json!({
            "taskName": "implement transport",
            "objective": "use one canonical transport",
            "scope": {
                "inScope": ["model routing"],
                "outOfScope": [],
                "scopeHints": ["code/pl-model"]
            },
            "implementationSteps": [{
                "id": "step-1",
                "instruction": "update routing",
                "targets": [{"path": "code/pl-model/src/lib.rs", "symbol": "route"}],
                "expectedOutcome": "one canonical route",
                "criterionIds": ["criterion-1"]
            }],
            "acceptanceCriteria": [{
                "id": "criterion-1",
                "requirement": "routing is canonical"
            }],
            "dependencies": [],
            "evidence": [],
            "verification": {
                "commands": [{
                    "id": "check-1",
                    "command": "cargo test -p pl-model",
                    "cwd": ".",
                    "purpose": "test routing",
                    "expectedOutcome": "tests pass",
                    "criterionIds": ["criterion-1"]
                }],
                "inspections": []
            }
        });
        assert!(
            serde_json::from_value::<TaskSpawnExecutorInput>(input.clone())
                .unwrap()
                .into_blueprint()
                .is_ok()
        );
        input["message"] = serde_json::json!("legacy duplicate instructions");
        assert!(serde_json::from_value::<TaskSpawnExecutorInput>(input).is_err());
    }

    #[test]
    fn executor_reuse_requires_the_complete_blueprint_fingerprint() {
        assert!(ensure_reused_blueprint_matches("sha256:same", "sha256:same").is_ok());
        assert_eq!(
            ensure_reused_blueprint_matches("sha256:steps-a", "sha256:steps-b")
                .unwrap_err()
                .to_string(),
            "executor allocation conflicts with the existing implementation blueprint"
        );
    }
}

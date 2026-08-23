//! 离线 Task Driver 验收夹具的脚本化 provider 状态机。
//!
//! 按域拆分:`planner`/`executor`/`reviewer` 承载各角色的分步响应脚本,
//! `request` 解析模型请求中的工具与输出,`worktree` 提供 fixture 的 Git 观测。

mod executor;
mod planner;
mod request;
mod reviewer;
mod worktree;

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use executor::executor_response;
use planner::planner_response;
use request::{find_string_field, function_call_outputs, parse_output, tool_name};
use reviewer::reviewer_response;

pub(super) const RECOVERY_INTERRUPTION_ACTION: &str = "hold Planner Turn for harness interruption";
pub(super) const BUDGET_COMPACTION_ACTION: &str = "hang executor rollover compaction";
pub(super) const BUDGET_RECOVERY_ACTION: &str = "hold budget NeedsAttention before send_message";
pub(super) const BUDGET_RESUMED_ACTION: &str = "hold resumed executor at budget slice one";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScriptRole {
    Planner,
    Executor,
    Reviewer,
}

impl ScriptRole {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Executor => "executor",
            Self::Reviewer => "reviewer",
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct ScriptProgress {
    pub(super) planner: usize,
    pub(super) executor: usize,
    pub(super) reviewer: usize,
    pub(super) executor_agent_id: Option<String>,
    pub(super) compaction_trigger_count: usize,
    pub(super) compaction_hung: bool,
    pub(super) budget_recovery_message_sent: bool,
    pub(super) budget_resumed_turn_seen: bool,
    task_revision: Option<u64>,
    task_generation: Option<u64>,
    expected_previous_head: Option<String>,
}

pub(super) fn role(request: &serde_json::Value) -> Result<ScriptRole> {
    let tools = request
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .context("model request has no tools array")?;
    let names = tools.iter().filter_map(tool_name).collect::<BTreeSet<_>>();
    if names.contains("report_completion") {
        return Ok(ScriptRole::Executor);
    }
    if names.contains("review_exit") {
        return Ok(ScriptRole::Reviewer);
    }
    if names.contains("task_status") || names.contains("task_transition") {
        return Ok(ScriptRole::Planner);
    }
    bail!("cannot identify scripted role from tools: {names:?}")
}

pub(super) fn observe_request(progress: &mut ScriptProgress, request: &serde_json::Value) {
    for output in function_call_outputs(request).filter_map(parse_output) {
        if progress.executor_agent_id.is_none() {
            progress.executor_agent_id = find_string_field(&output, "agentId");
        }
        let task = if let Some(task) = output.get("task") {
            task
        } else if output.get("generation").is_some() {
            &output
        } else {
            continue;
        };
        if let Some(revision) = task.get("revision").and_then(serde_json::Value::as_u64) {
            progress.task_revision = Some(revision);
        }
        if let Some(generation) = task.get("generation").and_then(serde_json::Value::as_u64) {
            progress.task_generation = Some(generation);
        }
    }
}

pub(super) fn next_step(progress: &mut ScriptProgress, role: ScriptRole) -> usize {
    let next = match role {
        ScriptRole::Planner => &mut progress.planner,
        ScriptRole::Executor => &mut progress.executor,
        ScriptRole::Reviewer => &mut progress.reviewer,
    };
    let step = *next;
    *next += 1;
    step
}

pub(super) fn response(
    progress: &mut ScriptProgress,
    workspace: &Path,
    role: ScriptRole,
    step: usize,
    exercise_recovery: bool,
    exercise_budget_recovery: bool,
) -> Result<(&'static str, String)> {
    match role {
        ScriptRole::Planner => planner_response(
            progress,
            workspace,
            step,
            exercise_recovery,
            exercise_budget_recovery,
        ),
        ScriptRole::Executor => executor_response(
            progress,
            workspace,
            step,
            exercise_recovery,
            exercise_budget_recovery,
        ),
        ScriptRole::Reviewer => reviewer_response(step),
    }
}

pub(super) fn is_compaction_request(request: &serde_json::Value) -> bool {
    request
        .get("input")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(serde_json::Value::as_str) == Some("compaction_trigger")
            })
        })
}

fn executor_id(progress: &ScriptProgress) -> Result<&str> {
    progress
        .executor_agent_id
        .as_deref()
        .context("executor agent id is not present in the durable spawn result")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function_call(body: &str) -> (String, serde_json::Value) {
        body.lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter(|line| *line != "[DONE]")
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find_map(|event| {
                (event.get("type").and_then(serde_json::Value::as_str)
                    == Some("response.output_item.done"))
                .then(|| event.get("item").cloned())
                .flatten()
                .filter(|item| {
                    item.get("type").and_then(serde_json::Value::as_str) == Some("function_call")
                })
            })
            .map(|item| {
                let name = item
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap()
                    .to_string();
                let arguments = item
                    .get("arguments")
                    .and_then(serde_json::Value::as_str)
                    .map(|arguments| serde_json::from_str(arguments).unwrap())
                    .unwrap();
                (name, arguments)
            })
            .unwrap()
    }

    #[test]
    fn observes_executor_id_from_responses_function_call_output() {
        let request = serde_json::json!({
            "input": [{
                "type": "function_call_output",
                "output": r#"{"agentId":"agent-responses"}"#
            }]
        });
        let mut progress = ScriptProgress::default();

        observe_request(&mut progress, &request);

        assert_eq!(
            progress.executor_agent_id.as_deref(),
            Some("agent-responses")
        );
    }

    #[test]
    fn observes_executor_id_from_chat_completions_tool_message() {
        let request = serde_json::json!({
            "messages": [{
                "role": "tool",
                "content": r#"{"agentId":"agent-chat"}"#,
                "tool_call_id": "spawn-executor"
            }]
        });
        let mut progress = ScriptProgress::default();

        observe_request(&mut progress, &request);

        assert_eq!(progress.executor_agent_id.as_deref(), Some("agent-chat"));
    }

    #[test]
    fn identifies_remote_compaction_without_consuming_a_role_step() {
        let request = serde_json::json!({
            "input": [
                {"type": "message", "role": "user", "content": "context"},
                {"type": "compaction_trigger"}
            ],
            "tools": []
        });
        let mut progress = ScriptProgress::default();

        assert!(is_compaction_request(&request));
        assert_eq!(progress.executor, 0);
        assert_eq!(next_step(&mut progress, ScriptRole::Executor), 0);
    }

    #[test]
    fn budget_recovery_ends_waiting_turn_then_resumes_from_planner_wake() {
        let mut progress = ScriptProgress {
            executor_agent_id: Some("executor-original".to_string()),
            ..ScriptProgress::default()
        };

        let (action, body) =
            planner_response(&mut progress, Path::new("."), 9, false, true).unwrap();
        assert_eq!(action, "final(budget NeedsAttention observed)");
        assert!(body.contains("Waiting for the queued Planner wake"));

        let expected = [
            (10, "task_status", serde_json::json!({})),
            (
                11,
                "send_message",
                serde_json::json!({
                    "target": "executor-original",
                    "message": "Resume the same WorkUnit and worktree. This message refreshes your budget; continue the original implementation and report completion without spawning replacement work."
                }),
            ),
            (12, "task_status", serde_json::json!({})),
            (
                13,
                "wait_agents",
                serde_json::json!({"targets": ["executor-original"]}),
            ),
            (
                14,
                "wait_agents",
                serde_json::json!({"targets": ["executor-original"]}),
            ),
            (
                15,
                "wait_agents",
                serde_json::json!({"targets": ["executor-original"]}),
            ),
            (
                16,
                "wait_agents",
                serde_json::json!({"targets": ["executor-original"]}),
            ),
            (17, "task_status", serde_json::json!({})),
            (
                18,
                "task_request_delivery_review",
                serde_json::json!({"executorAgentId": "executor-original"}),
            ),
            (19, "list_agents", serde_json::json!({})),
        ];

        for (step, expected_name, expected_arguments) in expected {
            let (_, body) =
                planner_response(&mut progress, Path::new("."), step, false, true).unwrap();
            let (name, arguments) = function_call(&body);
            assert_eq!(name, expected_name);
            assert_eq!(arguments, expected_arguments);
        }
        assert!(progress.budget_recovery_message_sent);
    }
}

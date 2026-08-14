use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::sse::{final_text, tool_call};

pub(super) const RECOVERY_INTERRUPTION_ACTION: &str = "hold Planner Turn for harness interruption";

const INITIAL_DESIGN_PATCH: &str = r#"*** Begin Patch
*** Add File: design/task-flow.md
+# Offline Task Flow
+
+The implementation must create `src/feature.txt` containing exactly `offline integration verified` followed by a newline.
*** End Patch"#;
const CONSISTENCY_DESIGN_PATCH: &str = r#"*** Begin Patch
*** Update File: design/task-flow.md
@@
 The implementation must create `src/feature.txt` containing exactly `offline integration verified` followed by a newline.
+
+Implementation status: completed and merged.
*** End Patch"#;
const FEATURE_PATCH: &str = r#"*** Begin Patch
*** Add File: src/feature.txt
+offline integration verified
*** End Patch"#;
const EXPECTED_PATCH_FAILURE: &str = r#"*** Begin Patch
*** Update File: README.md
@@
-# context that intentionally does not exist
+# replacement must never be applied
*** End Patch"#;

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
pub(super) struct ScriptProgress {
    pub(super) planner: usize,
    pub(super) executor: usize,
    pub(super) reviewer: usize,
    pub(super) executor_agent_id: Option<String>,
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
    if names.contains("plan_exit") || names.contains("task_update_design") {
        return Ok(ScriptRole::Planner);
    }
    bail!("cannot identify scripted role from tools: {names:?}")
}

pub(super) fn observe_request(progress: &mut ScriptProgress, request: &serde_json::Value) {
    if progress.executor_agent_id.is_none() {
        progress.executor_agent_id = function_call_outputs(request)
            .filter_map(parse_output)
            .find_map(|output| find_string_field(&output, "agentId"));
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
) -> Result<(&'static str, String)> {
    match role {
        ScriptRole::Planner => planner_response(progress, workspace, step, exercise_recovery),
        ScriptRole::Executor => executor_response(workspace, step, exercise_recovery),
        ScriptRole::Reviewer => reviewer_response(step),
    }
}

fn planner_response(
    progress: &mut ScriptProgress,
    workspace: &Path,
    mut step: usize,
    exercise_recovery: bool,
) -> Result<(&'static str, String)> {
    if exercise_recovery {
        match step {
            7 => {
                return Ok((
                    "final(executor failure observed)",
                    final_text(
                        "executor-failure-observed",
                        "The executor Turn failed after preserving workspace changes. Waiting for the durable Planner wake before pausing.",
                    ),
                ));
            }
            8 => {
                return Ok((
                    RECOVERY_INTERRUPTION_ACTION,
                    final_text(
                        "task-paused-for-recovery",
                        "The failure is durably recorded. The Task is paused for explicit recovery.",
                    ),
                ));
            }
            9 => {
                return Ok((
                    "task_status(after recovery)",
                    tool_call(
                        "status-after-recovery",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            10 => {
                let executor_id = executor_id(progress)?;
                return Ok((
                    "send_message(recovered executor)",
                    tool_call(
                        "resume-recovered-executor",
                        "send_message",
                        serde_json::json!({
                            "target": executor_id,
                            "message": "Continue in the same WorkUnit and worktree. Preserve the existing file change, commit it, verify it, and report completion."
                        }),
                    ),
                ));
            }
            11.. => step -= 5,
            0..=6 => {}
        }
    }
    let response = match step {
        0 => (
            "exec(planner rg --files)",
            tool_call(
                "planner-explore-files",
                "exec",
                serde_json::json!({"command": "rg --files"}),
            ),
        ),
        1 => (
            "plan_exit",
            tool_call(
                "plan",
                "plan_exit",
                serde_json::json!({
                    "content": "# Offline Task Driver plan\n\n1. Record the durable contract in `design/task-flow.md`.\n2. Spawn a fresh executor worktree and implement `src/feature.txt`.\n3. Review the delivery, merge it, perform integrated review, and complete the Task."
                }),
            ),
        ),
        2 => (
            "final",
            final_text("plan-submitted", "Plan submitted for confirmation."),
        ),
        3 => (
            "task_update_design(initial)",
            tool_call(
                "design-initial",
                "task_update_design",
                serde_json::json!({"patch": INITIAL_DESIGN_PATCH}),
            ),
        ),
        4 => (
            "task_spawn_executor",
            tool_call(
                "spawn-executor",
                "task_spawn_executor",
                serde_json::json!({
                    "taskName": "offline_executor",
                    "message": "Create src/feature.txt with the exact required content, commit it, verify it, and report the completion for review.",
                    "scopeHints": ["design"],
                    "acceptanceCriteria": ["src/feature.txt contains the exact required line", "all changes are committed"],
                    "evidence": [{"path": "design/task-flow.md", "line": 1, "symbol": "Offline Task Flow"}],
                    "verificationCommands": [{
                        "command": "git diff --check",
                        "cwd": ".",
                        "purpose": "verify the patch has no whitespace errors"
                    }]
                }),
            ),
        ),
        5..=8 => {
            let executor_id = executor_id(progress)?;
            (
                "wait_agents(executor)",
                tool_call(
                    &format!("wait-executor-{step}"),
                    "wait_agents",
                    serde_json::json!({"targets": [executor_id]}),
                ),
            )
        }
        9 => (
            "task_status(completion)",
            tool_call("status-completion", "task_status", serde_json::json!({})),
        ),
        10 => {
            let executor_id = executor_id(progress)?;
            (
                "task_request_delivery_review",
                tool_call(
                    "request-delivery-review",
                    "task_request_delivery_review",
                    serde_json::json!({"executorAgentId": executor_id}),
                ),
            )
        }
        11 => (
            "list_agents(delivery-review)",
            tool_call("list-delivery-review", "list_agents", serde_json::json!({})),
        ),
        12 => (
            "task_status(delivery-review)",
            tool_call(
                "status-delivery-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        13 => {
            let executor_id = executor_id(progress)?;
            (
                "close_agent(executor)",
                tool_call(
                    "close-executor",
                    "close_agent",
                    serde_json::json!({"target": executor_id}),
                ),
            )
        }
        14 => (
            "task_status(after executor close)",
            tool_call(
                "status-after-executor-close",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        15 => {
            let branch = task_worktree(workspace)?.branch;
            progress.expected_previous_head = Some(git_output(workspace, &["rev-parse", "HEAD"])?);
            (
                "exec(planner git merge)",
                tool_call(
                    "merge-executor",
                    "exec",
                    serde_json::json!({
                        "command": format!(
                            "git -c user.name=\"Pure Studio\" -c user.email=pure-studio@local merge --no-ff {branch} -m \"test: integrate offline task fixture\""
                        )
                    }),
                ),
            )
        }
        16 => {
            let executor_id = executor_id(progress)?;
            let previous = progress
                .expected_previous_head
                .as_deref()
                .context("merge base was not captured")?;
            let resulting = git_output(workspace, &["rev-parse", "HEAD"])?;
            (
                "task_record_merge",
                tool_call(
                    "record-executor-merge",
                    "task_record_merge",
                    serde_json::json!({
                        "executorAgentId": executor_id,
                        "completionRevision": 1,
                        "expectedPreviousHead": previous,
                        "resultingHead": resulting,
                        "method": "merge",
                        "summary": "Planner merged the approved offline executor branch with ordinary Git."
                    }),
                ),
            )
        }
        17 => (
            "task_update_design(consistency)",
            tool_call(
                "design-consistency",
                "task_update_design",
                serde_json::json!({"patch": CONSISTENCY_DESIGN_PATCH}),
            ),
        ),
        18 => (
            "task_request_integrated_review",
            tool_call(
                "request-integrated-review",
                "task_request_integrated_review",
                serde_json::json!({}),
            ),
        ),
        19 => (
            "list_agents(integrated-review)",
            tool_call(
                "list-integrated-review",
                "list_agents",
                serde_json::json!({}),
            ),
        ),
        20 => (
            "task_status(integrated-review)",
            tool_call(
                "status-integrated-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        21 => (
            "task_complete",
            tool_call("complete-task", "task_complete", serde_json::json!({})),
        ),
        _ => bail!("unexpected planner request step {step}"),
    };
    Ok(response)
}

fn executor_response(
    workspace: &Path,
    step: usize,
    exercise_recovery: bool,
) -> Result<(&'static str, String)> {
    let response = match step {
        0 => progress(
            "exploring",
            "Located the durable handoff and design contract.",
            "Create the required feature file.",
        ),
        1 => (
            "exec(expected failure)",
            tool_call(
                "executor-command-failure",
                "exec",
                serde_json::json!({
                    "command": "git rev-parse --verify refs/heads/__pure_fixture_missing__"
                }),
            ),
        ),
        2 => (
            "exec(corrected command)",
            tool_call(
                "executor-command-correction",
                "exec",
                serde_json::json!({"command": "git rev-parse --show-toplevel"}),
            ),
        ),
        3 => (
            "apply_patch(expected failure)",
            tool_call(
                "executor-patch-failure",
                "apply_patch",
                serde_json::json!({"input": EXPECTED_PATCH_FAILURE}),
            ),
        ),
        4 => (
            "read_file(after patch failure)",
            tool_call(
                "executor-read-after-patch-failure",
                "read_file",
                serde_json::json!({"path": "README.md"}),
            ),
        ),
        5 => (
            "apply_patch",
            tool_call(
                "executor-patch",
                "apply_patch",
                serde_json::json!({"input": FEATURE_PATCH}),
            ),
        ),
        6 if exercise_recovery => (
            "inject executor Turn failure",
            "data: this-is-not-json\n\n".to_string(),
        ),
        6 => progress(
            "implementing",
            "Corrected the command and patch failures, then created the required file.",
            "Commit and verify the change.",
        ),
        7 => (
            "exec(git add)",
            tool_call(
                "executor-add",
                "exec",
                serde_json::json!({"command": "git add -- src/feature.txt"}),
            ),
        ),
        8 => (
            "exec(git commit)",
            tool_call(
                "executor-commit",
                "exec",
                serde_json::json!({
                    "command": "git -c user.name=Pure -c user.email=pure@local commit -m \"test: add offline task fixture\""
                }),
            ),
        ),
        9 => progress(
            "verifying",
            "Committed and verified the exact worktree change.",
            "Report the completion for review.",
        ),
        10 => {
            let worktree = task_worktree(workspace)?;
            let head = git_output(&worktree.path, &["rev-parse", "HEAD"])?;
            (
                "report_completion",
                tool_call(
                    "executor-completion",
                    "report_completion",
                    serde_json::json!({
                        "kind": "delivery",
                        "headCommit": head,
                        "verificationSummary": "offline fixture content committed"
                    }),
                ),
            )
        }
        _ => bail!("unexpected executor request step {step}"),
    };
    Ok(response)
}

fn reviewer_response(step: usize) -> Result<(&'static str, String)> {
    let response = match step % 3 {
        0 => (
            "list_files(design)",
            tool_call(
                &format!("review-list-design-{step}"),
                "list_files",
                serde_json::json!({"path": "design"}),
            ),
        ),
        1 => (
            "read_file(design)",
            tool_call(
                &format!("review-read-design-{step}"),
                "read_file",
                serde_json::json!({"path": "design/task-flow.md"}),
            ),
        ),
        2 => (
            "review_exit(pass)",
            tool_call(
                &format!("review-pass-{step}"),
                "review_exit",
                serde_json::json!({
                    "verdict": "pass",
                    "summary": "Implementation matches the reviewed offline Task contract.",
                    "designReferences": [{"path": "design/task-flow.md", "section": "Offline Task Flow"}],
                    "findings": [],
                    "fileReviews": [{"path": "src/feature.txt", "reviewed": true}]
                }),
            ),
        ),
        _ => unreachable!(),
    };
    Ok(response)
}

fn progress(stage: &str, summary: &str, next_step: &str) -> (&'static str, String) {
    (
        "report_progress",
        tool_call(
            &format!("executor-progress-{stage}"),
            "report_progress",
            serde_json::json!({"stage": stage, "summary": summary, "nextStep": next_step}),
        ),
    )
}

fn executor_id(progress: &ScriptProgress) -> Result<&str> {
    progress
        .executor_agent_id
        .as_deref()
        .context("executor agent id is not present in the durable spawn result")
}

fn tool_name(tool: &serde_json::Value) -> Option<&str> {
    tool.get("name")
        .or_else(|| {
            tool.get("function")
                .and_then(|function| function.get("name"))
        })
        .and_then(serde_json::Value::as_str)
}

fn function_call_outputs(request: &serde_json::Value) -> impl Iterator<Item = &str> {
    let responses_outputs = request
        .get("input")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .rev()
        .filter(|item| {
            item.get("type").and_then(serde_json::Value::as_str) == Some("function_call_output")
        })
        .filter_map(|item| item.get("output").and_then(serde_json::Value::as_str));
    let chat_completions_outputs = request
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .rev()
        .filter(|message| message.get("role").and_then(serde_json::Value::as_str) == Some("tool"))
        .filter_map(|message| message.get("content").and_then(serde_json::Value::as_str));

    responses_outputs.chain(chat_completions_outputs)
}

fn parse_output(output: &str) -> Option<serde_json::Value> {
    serde_json::from_str(output).ok().or_else(|| {
        let start = output.find('{')?;
        let end = output.rfind('}')?;
        serde_json::from_str(&output[start..=end]).ok()
    })
}

fn find_string_field(value: &serde_json::Value, field: &str) -> Option<String> {
    match value {
        serde_json::Value::Object(object) => object
            .get(field)
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                object
                    .values()
                    .find_map(|value| find_string_field(value, field))
            }),
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|value| find_string_field(value, field)),
        _ => None,
    }
}

struct TaskWorktree {
    path: PathBuf,
    branch: String,
}

fn task_worktree(workspace: &Path) -> Result<TaskWorktree> {
    let output = git_output(workspace, &["worktree", "list", "--porcelain"])?;
    let mut path = None;
    for line in output.lines().chain(std::iter::once("")) {
        if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(value));
            continue;
        }
        if let Some(branch) = line.strip_prefix("branch refs/heads/")
            && branch.starts_with("pure-task-")
        {
            return Ok(TaskWorktree {
                path: path.context("Task worktree entry has no path")?,
                branch: branch.to_string(),
            });
        }
    }
    bail!("Task worktree is absent from git worktree list")
}

fn git_output(workspace: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(workspace).args(args);
    pl_studio_runtime::process::configure_background_std_command(&mut command);
    let output = command
        .output()
        .with_context(|| format!("failed to execute git {:?}", args))?;
    if !output.status.success() {
        bail!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

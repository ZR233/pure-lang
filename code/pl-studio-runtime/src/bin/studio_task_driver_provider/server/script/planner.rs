//! Planner 角色的分步响应脚本。

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::super::sse::{final_text, tool_call};
use super::worktree::{git_output, task_worktree};
use super::{BUDGET_RECOVERY_ACTION, RECOVERY_INTERRUPTION_ACTION, ScriptProgress, executor_id};

const INITIAL_DESIGN_PATCH: &str = r#"*** Begin Patch
*** Add File: design/task-flow.md
+# Offline Task Flow
+
+The implementation must create `src/feature.txt` containing exactly `offline integration verified` followed by a newline.
*** End Patch"#;

pub(super) fn planner_response(
    progress: &mut ScriptProgress,
    workspace: &Path,
    mut step: usize,
    exercise_recovery: bool,
    exercise_budget_recovery: bool,
) -> Result<(&'static str, String)> {
    if exercise_recovery {
        match step {
            8 => {
                return Ok((
                    "final(executor failure observed)",
                    final_text(
                        "executor-failure-observed",
                        "The executor Turn failed after preserving workspace changes. Waiting for the durable Planner wake before pausing.",
                    ),
                ));
            }
            9 => {
                return Ok((
                    RECOVERY_INTERRUPTION_ACTION,
                    final_text(
                        "task-paused-for-recovery",
                        "The failure is durably recorded. The Task is paused for explicit recovery.",
                    ),
                ));
            }
            10 => {
                return Ok((
                    "task_status(after recovery)",
                    tool_call(
                        "status-after-recovery",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            11 => {
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
            12.. => step -= 5,
            0..=7 => {}
        }
    }
    if exercise_budget_recovery {
        match step {
            7 => {
                return Ok((
                    "final(budget NeedsAttention observed)",
                    final_text(
                        "budget-needs-attention-observed",
                        "The executor budget terminal is durable. Waiting for the queued Planner wake before recovering the same executor.",
                    ),
                ));
            }
            8 => {
                return Ok((
                    "task_status(budget NeedsAttention)",
                    tool_call(
                        "status-budget-needs-attention",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            9 => {
                progress.budget_recovery_message_sent = true;
                let executor_id = executor_id(progress)?;
                return Ok((
                    BUDGET_RECOVERY_ACTION,
                    tool_call(
                        "refresh-budgeted-executor",
                        "send_message",
                        serde_json::json!({
                            "target": executor_id,
                            "message": "Resume the same WorkUnit and worktree. This message refreshes your budget; continue the original implementation and report completion without spawning replacement work."
                        }),
                    ),
                ));
            }
            10 => {
                return Ok((
                    "task_status(recovered running)",
                    tool_call(
                        "status-recovered-running",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            11..=14 => {
                let executor_id = executor_id(progress)?;
                return Ok((
                    "wait_agents(recovered executor)",
                    tool_call(
                        &format!("wait-recovered-executor-{step}"),
                        "wait_agents",
                        serde_json::json!({"targets": [executor_id]}),
                    ),
                ));
            }
            15 => {
                return Ok((
                    "task_status(recovered completion)",
                    tool_call(
                        "status-recovered-completion",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            16 => {
                let executor_id = executor_id(progress)?;
                return Ok((
                    "task_request_delivery_review",
                    tool_call(
                        "request-recovered-delivery-review",
                        "task_request_delivery_review",
                        serde_json::json!({"executorAgentId": executor_id}),
                    ),
                ));
            }
            17.. => step -= 5,
            0..=6 => {}
        }
    }
    let response = match step {
        0 => (
            "list_files(planner workspace)",
            tool_call(
                "planner-explore-files",
                "list_files",
                serde_json::json!({"path": ".", "limit": 100}),
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
            "apply_patch(initial design)",
            tool_call(
                "design-initial",
                "apply_patch",
                serde_json::json!({"input": INITIAL_DESIGN_PATCH, "cwd": "."}),
            ),
        ),
        4 => (
            "task_finalize_design",
            tool_call(
                "finalize-design",
                "task_finalize_design",
                serde_json::json!({"summary": "Recorded the executor contract in design/task-flow.md."}),
            ),
        ),
        5 => (
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
        6..=9 => {
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
        10 => (
            "task_status(completion)",
            tool_call("status-completion", "task_status", serde_json::json!({})),
        ),
        11 => {
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
        12 => (
            "list_agents(delivery-review)",
            tool_call("list-delivery-review", "list_agents", serde_json::json!({})),
        ),
        13 => (
            "task_status(delivery-review)",
            tool_call(
                "status-delivery-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        14 => {
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
        15 => (
            "task_status(after executor close)",
            tool_call(
                "status-after-executor-close",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        16 => {
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
        17 => {
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

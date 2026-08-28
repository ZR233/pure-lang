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
            10 => {
                return Ok((
                    "final(executor failure observed)",
                    final_text(
                        "executor-failure-observed",
                        "The executor Turn failed after preserving workspace changes. Waiting for the durable Planner wake before pausing.",
                    ),
                ));
            }
            11 => {
                return Ok((
                    RECOVERY_INTERRUPTION_ACTION,
                    final_text(
                        "task-waiting-for-continuation",
                        "The failure is durably recorded. The Task state is unchanged and can continue with an ordinary new message.",
                    ),
                ));
            }
            12 => {
                return Ok((
                    "task_status(after recovery)",
                    tool_call(
                        "status-after-recovery",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            13 => {
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
            14.. => step -= 6,
            0..=9 => {}
        }
    }
    if exercise_budget_recovery {
        match step {
            9 => {
                return Ok((
                    "final(budget NeedsAttention observed)",
                    final_text(
                        "budget-needs-attention-observed",
                        "The executor budget terminal is durable. Waiting for the queued Planner wake before recovering the same executor.",
                    ),
                ));
            }
            10 => {
                return Ok((
                    "task_status(budget NeedsAttention)",
                    tool_call(
                        "status-budget-needs-attention",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            11 => {
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
            12 => {
                return Ok((
                    "task_status(recovered running)",
                    tool_call(
                        "status-recovered-running",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            13..=16 => {
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
            17 => {
                return Ok((
                    "task_status(recovered completion)",
                    tool_call(
                        "status-recovered-completion",
                        "task_status",
                        serde_json::json!({}),
                    ),
                ));
            }
            18 => {
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
            19.. => step -= 5,
            0..=8 => {}
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
            "task_status(planning)",
            tool_call("status-planning", "task_status", serde_json::json!({})),
        ),
        2 => (
            "task_transition(submit-plan)",
            transition_call(
                progress,
                "submit-plan",
                "submitPlan",
                serde_json::json!({
                    "summary": "# Offline Task Driver plan\n\n1. Record the durable contract in `design/task-flow.md`.\n2. Spawn a fresh executor worktree and implement `src/feature.txt`.\n3. Review the delivery, merge it, perform integrated review, and complete the Task."
                }),
            )?,
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
            "exec(commit design)",
            tool_call(
                "commit-design",
                "exec",
                serde_json::json!({
                    "command": "git add -- design/task-flow.md && git -c user.name=\"Pure Studio\" -c user.email=pure-studio@local commit -m \"docs: record offline Task contract\""
                }),
            ),
        ),
        5 => (
            "task_status(editing-documents)",
            tool_call(
                "status-editing-documents",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        6 => (
            "task_transition(finish-document-editing)",
            transition_call(
                progress,
                "finish-document-editing",
                "finishDocumentEditing",
                serde_json::json!({"summary": "Recorded the executor contract in design/task-flow.md."}),
            )?,
        ),
        7 => (
            "task_spawn_executor",
            tool_call(
                "spawn-executor",
                "task_spawn_executor",
                serde_json::json!({
                    "taskName": "offline_executor",
                    "objective": "Create and commit src/feature.txt with the exact fixture content.",
                    "scope": {
                        "inScope": ["Create the deterministic feature fixture and verify its committed diff."],
                        "outOfScope": ["Do not change the Task design or merge the executor branch."],
                        "scopeHints": ["src/feature.txt"]
                    },
                    "implementationSteps": [{
                        "id": "step-create",
                        "instruction": "Create src/feature.txt with the exact fixture content from the confirmed plan.",
                        "targets": [{"path": "src/feature.txt"}],
                        "expectedOutcome": "The executor worktree contains the exact deterministic fixture file.",
                        "criterionIds": ["criterion-content"]
                    }, {
                        "id": "step-commit",
                        "instruction": "Commit the feature file and leave the executor worktree clean.",
                        "targets": [{"path": "src/feature.txt"}],
                        "expectedOutcome": "HEAD contains the feature file and the worktree is clean.",
                        "criterionIds": ["criterion-commit"]
                    }],
                    "acceptanceCriteria": [{
                        "id": "criterion-content",
                        "requirement": "src/feature.txt has the exact required fixture content."
                    }, {
                        "id": "criterion-commit",
                        "requirement": "The delivery commit is clean and has no whitespace errors."
                    }],
                    "dependencies": [],
                    "evidence": [{
                        "path": "design/task-flow.md",
                        "symbol": "Offline Task Flow",
                        "note": "Confirmed design contract for the fixture."
                    }],
                    "verification": {
                        "commands": [{
                            "id": "check-diff",
                            "command": "git diff --check HEAD^ HEAD",
                            "cwd": ".",
                            "purpose": "Verify the committed patch has no whitespace errors.",
                            "expectedOutcome": "The command exits successfully with no output.",
                            "criterionIds": ["criterion-commit"]
                        }],
                        "inspections": [{
                            "id": "inspect-feature",
                            "instruction": "Inspect src/feature.txt and compare it with the confirmed fixture content.",
                            "targets": [{"path": "src/feature.txt"}],
                            "expectedOutcome": "The file content matches the confirmed contract exactly.",
                            "criterionIds": ["criterion-content"]
                        }]
                    }
                }),
            ),
        ),
        8..=11 => {
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
        12 => (
            "task_status(completion)",
            tool_call("status-completion", "task_status", serde_json::json!({})),
        ),
        13 => {
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
        14 => (
            "list_agents(delivery-review)",
            tool_call("list-delivery-review", "list_agents", serde_json::json!({})),
        ),
        15 => (
            "task_status(delivery-review)",
            tool_call(
                "status-delivery-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        16 => {
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
        17 => (
            "task_status(after executor close)",
            tool_call(
                "status-after-executor-close",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        18 => {
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
        19 => {
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
        20 => (
            "task_status(review-gate)",
            tool_call("status-review-gate", "task_status", serde_json::json!({})),
        ),
        21 => (
            "task_transition(begin-integrated-review)",
            transition_call(
                progress,
                "begin-integrated-review",
                "beginIntegratedReview",
                serde_json::json!({}),
            )?,
        ),
        22 => (
            "list_agents(integrated-review)",
            tool_call(
                "list-integrated-review",
                "list_agents",
                serde_json::json!({}),
            ),
        ),
        23 => (
            "task_status(integrated-review)",
            tool_call(
                "status-integrated-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        24 => (
            "task_transition(complete)",
            transition_call(
                progress,
                "complete-task",
                "complete",
                serde_json::json!({
                    "outcome": "succeeded",
                    "summary": "Offline Task Driver fixture completed after integrated review."
                }),
            )?,
        ),
        _ => bail!("unexpected planner request step {step}"),
    };
    Ok(response)
}

fn transition_call(
    progress: &ScriptProgress,
    call_id: &str,
    action: &str,
    fields: serde_json::Value,
) -> Result<String> {
    let mut input = fields
        .as_object()
        .cloned()
        .context("task_transition fixture fields must be an object")?;
    input.insert("action".to_string(), serde_json::json!(action));
    input.insert(
        "expectedRevision".to_string(),
        serde_json::json!(
            progress.task_revision.context(
                "task_transition requires a preceding task_status or accepted transition"
            )?
        ),
    );
    input.insert(
        "expectedGeneration".to_string(),
        serde_json::json!(
            progress.task_generation.context(
                "task_transition requires a preceding task_status or accepted transition"
            )?
        ),
    );
    Ok(tool_call(
        call_id,
        "task_transition",
        serde_json::Value::Object(input),
    ))
}

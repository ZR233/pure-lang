//! Executor 角色的分步响应脚本。

use std::path::Path;

use anyhow::{Result, bail};

use super::super::sse::tool_call;
use super::worktree::{git_output, task_worktree};
use super::{BUDGET_RESUMED_ACTION, ScriptProgress};

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

pub(super) fn executor_response(
    script_progress: &mut ScriptProgress,
    workspace: &Path,
    step: usize,
    exercise_recovery: bool,
    exercise_budget_recovery: bool,
) -> Result<(&'static str, String)> {
    let response = match step {
        0 => {
            let (_, body) = self::progress(
                "exploring",
                "Located the durable handoff and design contract.",
                "Create the required feature file.",
            );
            if exercise_budget_recovery {
                script_progress.budget_resumed_turn_seen = true;
                (BUDGET_RESUMED_ACTION, body)
            } else {
                ("report_progress", body)
            }
        }
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

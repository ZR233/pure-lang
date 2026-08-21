use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{StudioRuntime, StudioTaskRuntime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use super::git::git_output;
use super::sse::{final_text, repeated_tool_call, tool_call};
use super::{PARENT_HISTORY_MARKER, PLANNER_FOLLOWUP_PATH};

const INITIAL_DESIGN_PATCH: &str = r#"*** Begin Patch
*** Add File: design/task-flow.md
+# Offline Task Flow
+
+The implementation must create `src/feature.txt` containing exactly `offline integration verified` followed by a newline.
*** End Patch"#;
const FEATURE_PATCH: &str = r#"*** Begin Patch
*** Add File: src/feature.txt
+offline integration verified
*** End Patch"#;
const PLANNER_FOLLOWUP_PATCH: &str = r#"*** Begin Patch
*** Add File: src/planner-followup.txt
+planner merge adjustment verified
*** End Patch"#;
const EXPECTED_PATCH_FAILURE: &str = r#"*** Begin Patch
*** Update File: README.md
@@
-# context that intentionally does not exist
+# replacement must never be applied
*** End Patch"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptRole {
    Planner,
    Executor,
    Reviewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptMode {
    SingleExecutorEquivalent,
    PostMergeImplementation,
}

impl ScriptRole {
    fn label(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Executor => "executor",
            Self::Reviewer => "reviewer",
        }
    }
}

#[derive(Debug, Default)]
struct ScriptProgress {
    planner: usize,
    executor: usize,
    reviewer: usize,
    requests: Vec<String>,
    errors: Vec<String>,
}

struct ScriptState {
    runtime: StudioRuntime,
    thread_id: String,
    workspace: PathBuf,
    mode: ScriptMode,
    progress: Mutex<ScriptProgress>,
}

pub(super) struct ScriptedModelServer {
    state: Arc<ScriptState>,
    task: tokio::task::JoinHandle<()>,
}

impl ScriptedModelServer {
    pub(super) fn start(
        listener: TcpListener,
        runtime: StudioRuntime,
        thread_id: String,
        workspace: PathBuf,
        mode: ScriptMode,
    ) -> Self {
        let state = Arc::new(ScriptState {
            runtime,
            thread_id,
            workspace,
            mode,
            progress: Mutex::new(ScriptProgress::default()),
        });
        let server_state = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                let request_state = server_state.clone();
                tokio::spawn(async move {
                    serve_request(socket, request_state).await;
                });
            }
        });
        Self { state, task }
    }

    pub(super) async fn assert_complete(&self) -> Result<()> {
        let progress = self.state.progress.lock().await;
        if !progress.errors.is_empty() {
            bail!("scripted model errors:\n{}", progress.errors.join("\n"));
        }
        let expected = match self.state.mode {
            ScriptMode::SingleExecutorEquivalent => (19, 12, 3),
            ScriptMode::PostMergeImplementation => (24, 12, 7),
        };
        if (progress.planner, progress.executor, progress.reviewer) != expected {
            bail!(
                "scripted model stopped at planner={}, executor={}, reviewer={}; expected {expected:?}\n{}",
                progress.planner,
                progress.executor,
                progress.reviewer,
                progress.requests.join("\n")
            );
        }
        assert!(
            !progress
                .requests
                .iter()
                .any(|request| request.contains("list_agents(executor)")),
            "executor wait delta should not be followed by list_agents refresh"
        );
        if self.state.mode == ScriptMode::PostMergeImplementation {
            let sequence = progress.requests.join("\n");
            for expected_action in [
                "task_request_integrated_review",
                "wait_agents(integrated-review)",
                "task_status(integrated-review)",
                "task_complete",
            ] {
                assert!(
                    sequence.contains(expected_action),
                    "missing scripted action {expected_action}:\n{sequence}"
                );
            }
        }
        Ok(())
    }

    pub(super) async fn diagnostics(&self) -> String {
        let progress = self.state.progress.lock().await;
        format!(
            "model requests: planner={}, executor={}, reviewer={}\n{}\nerrors:\n{}",
            progress.planner,
            progress.executor,
            progress.reviewer,
            progress.requests.join("\n"),
            progress.errors.join("\n")
        )
    }

    pub(super) fn stop(&self) {
        self.task.abort();
    }
}

impl Drop for ScriptedModelServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn serve_request(mut socket: TcpStream, state: Arc<ScriptState>) {
    let result = async {
        let request = read_json_request(&mut socket).await?;
        let role = request_role(&request)?;
        let step = next_step(&state, role).await;
        validate_request_step(&request, role, step, state.mode)?;
        let (action, body) = scripted_response(&state, role, step)
            .await
            .with_context(|| {
                latest_function_call_output(&request).map_or_else(
                    || "model request has no prior function_call_output".to_string(),
                    |output| format!("latest function_call_output: {output}"),
                )
            })?;
        state
            .progress
            .lock()
            .await
            .requests
            .push(format!("{}[{step}] {action}", role.label()));
        write_response(&mut socket, "200 OK", "text/event-stream", &body).await
    }
    .await;

    if let Err(error) = result {
        state
            .progress
            .lock()
            .await
            .errors
            .push(format!("{error:#}"));
        let body = format!("scripted model request failed: {error:#}");
        let _ = write_response(&mut socket, "400 Bad Request", "text/plain", &body).await;
    }
}

async fn scripted_response(
    state: &ScriptState,
    role: ScriptRole,
    step: usize,
) -> Result<(&'static str, String)> {
    match role {
        ScriptRole::Planner => planner_response(state, step).await,
        ScriptRole::Executor => executor_response(state, step).await,
        ScriptRole::Reviewer => reviewer_response(state.mode, step),
    }
}

async fn planner_response(state: &ScriptState, step: usize) -> Result<(&'static str, String)> {
    let response = match step {
        0 => (
            "list_files(planner workspace)",
            tool_call(
                "planner-explore-files",
                "list_files",
                serde_json::json!({
                    "path": ".",
                    "limit": 100
                }),
            ),
        ),
        1 => (
            "plan_exit",
            tool_call(
                "plan",
                "plan_exit",
                serde_json::json!({
                    "content": "# Offline task plan\n\n1. Record the contract in `design/task-flow.md`.\n2. Implement in an executor worktree.\n3. Review the completion, merge it, then review the integrated Task HEAD."
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
            "task_spawn_executor(exact duplicate)",
            repeated_tool_call(
                "spawn-executor",
                "task_spawn_executor",
                serde_json::json!({
                    "taskName": "offline_executor",
                    "objective": "Create and commit src/feature.txt with the exact fixture content.",
                    "scope": {
                        "inScope": ["Create the deterministic feature fixture and verify its committed diff."],
                        "outOfScope": ["Do not change the Task design or merge the executor branch."],
                        "scopeHints": ["design"]
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
        6..=9 => {
            let executor_id = executor_agent_id(state).await?;
            (
                "wait_agents(executor)",
                tool_call(
                    "wait-executor",
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
            let executor_id = executor_agent_id(state).await?;
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
            let executor_id = executor_agent_id(state).await?;
            (
                "close_agent(executor)",
                tool_call(
                    "close-executor",
                    "close_agent",
                    serde_json::json!({"target": executor_id}),
                ),
            )
        }
        15 => {
            let task = current_task(state).await?;
            let work_unit = task
                .work_units
                .iter()
                .find(|unit| unit.agent_id.is_some())
                .context("executor work unit is absent before planner Git integration")?;
            (
                "exec(planner git merge)",
                tool_call(
                    "merge-executor",
                    "exec",
                    serde_json::json!({
                        "command": format!(
                            "git -c user.name=\"Pure Studio\" -c user.email=pure-studio@local merge --no-ff {} -m \"test: integrate offline task fixture\"",
                            work_unit.branch
                        )
                    }),
                ),
            )
        }
        16 if state.mode == ScriptMode::PostMergeImplementation => (
            "apply_patch(planner merge adjustment)",
            tool_call(
                "planner-merge-adjustment",
                "apply_patch",
                serde_json::json!({
                    "input": PLANNER_FOLLOWUP_PATCH,
                    "cwd": "."
                }),
            ),
        ),
        17 if state.mode == ScriptMode::PostMergeImplementation => (
            "exec(amend planner merge)",
            tool_call(
                "amend-planner-merge",
                "exec",
                serde_json::json!({
                    "command": "git add -- src/planner-followup.txt && git -c user.name=\"Pure Studio\" -c user.email=pure-studio@local commit --amend --no-edit"
                }),
            ),
        ),
        16 if state.mode == ScriptMode::SingleExecutorEquivalent => {
            ("task_record_merge", record_merge_call(state).await?)
        }
        18 if state.mode == ScriptMode::PostMergeImplementation => {
            ("task_record_merge", record_merge_call(state).await?)
        }
        17 if state.mode == ScriptMode::SingleExecutorEquivalent => (
            "task_status(review-gate)",
            tool_call("status-review-gate", "task_status", serde_json::json!({})),
        ),
        19 if state.mode == ScriptMode::PostMergeImplementation => (
            "task_status(review-gate)",
            tool_call("status-review-gate", "task_status", serde_json::json!({})),
        ),
        18 if state.mode == ScriptMode::SingleExecutorEquivalent => (
            "task_complete",
            tool_call("complete-task", "task_complete", serde_json::json!({})),
        ),
        20 if state.mode == ScriptMode::PostMergeImplementation => (
            "task_request_integrated_review",
            tool_call(
                "request-integrated-review",
                "task_request_integrated_review",
                serde_json::json!({}),
            ),
        ),
        21 if state.mode == ScriptMode::PostMergeImplementation => {
            let reviewer_id = integrated_reviewer_agent_id(state).await?;
            (
                "wait_agents(integrated-review)",
                tool_call(
                    "wait-integrated-review",
                    "wait_agents",
                    serde_json::json!({"targets": [reviewer_id]}),
                ),
            )
        }
        22 if state.mode == ScriptMode::PostMergeImplementation => (
            "task_status(integrated-review)",
            tool_call(
                "status-integrated-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        23 if state.mode == ScriptMode::PostMergeImplementation => (
            "task_complete",
            tool_call("complete-task", "task_complete", serde_json::json!({})),
        ),
        _ => bail!("unexpected planner request step {step}"),
    };
    Ok(response)
}

async fn record_merge_call(state: &ScriptState) -> Result<String> {
    let task = current_task(state).await?;
    let executor_id = executor_agent_id(state).await?;
    let completion = task
        .completions
        .iter()
        .filter(|completion| completion.executor_agent_id == executor_id)
        .max_by_key(|completion| completion.revision)
        .context("approved executor completion is absent before merge accounting")?;
    let resulting_head = git_output(&state.workspace, &["rev-parse", "HEAD"])?;
    if resulting_head == task.expected_head {
        bail!("planner Git merge did not advance the main workspace HEAD");
    }
    Ok(tool_call(
        "record-executor-merge",
        "task_record_merge",
        serde_json::json!({
            "executorAgentId": executor_id,
            "completionRevision": completion.revision,
            "expectedPreviousHead": task.expected_head,
            "resultingHead": resulting_head,
            "method": "merge",
            "summary": "Planner merged the approved offline executor branch with ordinary Git."
        }),
    ))
}

async fn executor_response(state: &ScriptState, step: usize) -> Result<(&'static str, String)> {
    let response = match step {
        0 => (
            "report_progress(exploring)",
            tool_call(
                "executor-progress-exploring",
                "report_progress",
                serde_json::json!({
                    "stage": "exploring",
                    "summary": "Located the canonical worktree and design contract.",
                    "nextStep": "Create the required feature file."
                }),
            ),
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
        6 => (
            "report_progress(implementing)",
            tool_call(
                "executor-progress-implementing",
                "report_progress",
                serde_json::json!({
                    "stage": "implementing",
                    "summary": "Corrected the command and patch failures, then created the required file.",
                    "nextStep": "Commit and verify the change."
                }),
            ),
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
        9 => (
            "exec(verification)",
            tool_call(
                "executor-verify-diff",
                "exec",
                serde_json::json!({"command": "git diff --check HEAD^ HEAD"}),
            ),
        ),
        10 => (
            "report_progress(verifying)",
            tool_call(
                "executor-progress-verifying",
                "report_progress",
                serde_json::json!({
                    "stage": "verifying",
                    "summary": "Committed the exact worktree change outside the review-focus hint.",
                    "nextStep": "Report the verified completion for review."
                }),
            ),
        ),
        11 => {
            let task = current_task(state).await?;
            let work_unit = task
                .work_units
                .iter()
                .find(|unit| unit.agent_id.is_some())
                .context("executor work unit is absent from task projection")?;
            let head = git_output(Path::new(&work_unit.worktree_path), &["rev-parse", "HEAD"])?;
            (
                "report_completion",
                tool_call(
                    "executor-completion",
                    "report_completion",
                    serde_json::json!({
                        "kind": "delivery",
                        "headCommit": head,
                        "verificationResults": [{
                            "checkId": "check-diff",
                            "summary": "git diff --check HEAD^ HEAD exited successfully with no output"
                        }, {
                            "checkId": "inspect-feature",
                            "summary": "src/feature.txt matches the confirmed fixture content exactly"
                        }]
                    }),
                ),
            )
        }
        _ => bail!("unexpected executor request step {step}"),
    };
    Ok(response)
}

fn reviewer_response(mode: ScriptMode, step: usize) -> Result<(&'static str, String)> {
    let response = match (mode, step) {
        (_, 0) => (
            "list_files(design)",
            tool_call(
                "review-list-design",
                "list_files",
                serde_json::json!({"path": "design"}),
            ),
        ),
        (_, 1) => (
            "read_file(design)",
            tool_call(
                "review-read-design",
                "read_file",
                serde_json::json!({"path": "design/task-flow.md"}),
            ),
        ),
        (_, 2) => ("review_exit(pass)", review_exit_call(false)),
        (ScriptMode::PostMergeImplementation, 3) => (
            "list_files(integrated)",
            tool_call(
                "integrated-list-files",
                "list_files",
                serde_json::json!({"path": "src"}),
            ),
        ),
        (ScriptMode::PostMergeImplementation, 4) => (
            "read_file(integrated-design)",
            tool_call(
                "integrated-read-design",
                "read_file",
                serde_json::json!({"path": "design/task-flow.md"}),
            ),
        ),
        (ScriptMode::PostMergeImplementation, 5) => (
            "read_file(integrated-change)",
            tool_call(
                "integrated-read-change",
                "read_file",
                serde_json::json!({"path": PLANNER_FOLLOWUP_PATH}),
            ),
        ),
        (ScriptMode::PostMergeImplementation, 6) => {
            ("review_exit(integrated-pass)", review_exit_call(true))
        }
        _ => bail!("unexpected reviewer request step {step}"),
    };
    Ok(response)
}

fn review_exit_call(integrated: bool) -> String {
    tool_call(
        if integrated {
            "integrated-review-pass"
        } else {
            "review-pass"
        },
        "review_exit",
        serde_json::json!({
            "verdict": "pass",
            "summary": if integrated {
                "Integrated implementation matches the final reviewed Task contract."
            } else {
                "Implementation matches the reviewed offline task contract."
            },
            "designReferences": [{
                "path": "design/task-flow.md",
                "section": "Offline Task Flow"
            }],
            "findings": [],
            "fileReviews": if integrated {
                serde_json::json!([{
                    "path": "src/feature.txt",
                    "reviewed": true
                }, {
                    "path": PLANNER_FOLLOWUP_PATH,
                    "reviewed": true
                }])
            } else {
                serde_json::json!([{
                    "path": "src/feature.txt",
                    "reviewed": true
                }])
            }
        }),
    )
}

async fn current_task(state: &ScriptState) -> Result<StudioTaskRuntime> {
    state
        .runtime
        .thread_task_view(&state.thread_id)
        .await?
        .context("task projection is not available")
}

async fn executor_agent_id(state: &ScriptState) -> Result<String> {
    current_task(state)
        .await?
        .work_units
        .into_iter()
        .find_map(|unit| unit.agent_id)
        .context("executor is absent from task projection")
}

async fn integrated_reviewer_agent_id(state: &ScriptState) -> Result<String> {
    current_task(state)
        .await?
        .reviews
        .into_iter()
        .filter(|review| review.scope == "integrated")
        .find_map(|review| review.reviewer_agent_id)
        .context("integrated reviewer is absent from task projection")
}

async fn next_step(state: &ScriptState, role: ScriptRole) -> usize {
    let mut progress = state.progress.lock().await;
    let next = match role {
        ScriptRole::Planner => &mut progress.planner,
        ScriptRole::Executor => &mut progress.executor,
        ScriptRole::Reviewer => &mut progress.reviewer,
    };
    let step = *next;
    *next += 1;
    step
}

fn request_role(request: &serde_json::Value) -> Result<ScriptRole> {
    let tools = request_tools(request)?;
    let names = tools
        .iter()
        .filter_map(|tool| {
            tool.get("name")
                .or_else(|| {
                    tool.get("function")
                        .and_then(|function| function.get("name"))
                })
                .and_then(serde_json::Value::as_str)
        })
        .collect::<BTreeSet<_>>();
    if names.contains("wait_agent") {
        bail!("model-visible wait_agent must not be installed");
    }
    if names.contains("report_completion") {
        ensure_fresh_task_child(request, "executor")?;
        return Ok(ScriptRole::Executor);
    }
    if names.contains("review_exit") {
        ensure_fresh_task_child(request, "reviewer")?;
        return Ok(ScriptRole::Reviewer);
    }
    if names.contains("plan_exit") || names.contains("task_status") {
        return Ok(ScriptRole::Planner);
    }
    bail!("cannot identify scripted role from tools: {names:?}")
}

fn validate_request_step(
    request: &serde_json::Value,
    role: ScriptRole,
    step: usize,
    mode: ScriptMode,
) -> Result<()> {
    if role == ScriptRole::Planner {
        let tools = request_tools(request)?;
        if step <= 2 {
            validate_planning_contract(tools)?;
        } else {
            validate_planner_spawn_contract(tools)?;
        }
    }
    if role == ScriptRole::Planner && step == 1 {
        let output = latest_function_call_output(request)
            .context("Planner list_files did not return a tool result")?;
        if !output.contains("README.md") {
            bail!(
                "Planner list_files was expected to observe README.md before plan_exit: {output}"
            );
        }
    }
    if role == ScriptRole::Planner
        && step == 6
        && !latest_function_call_output(request)
            .is_some_and(|output| output.contains("\"reused\":true"))
    {
        bail!("the repeated executor allocation did not resolve to the durable WorkUnit");
    }
    if mode == ScriptMode::SingleExecutorEquivalent
        && role == ScriptRole::Planner
        && step == 18
        && !latest_function_call_output(request).is_some_and(|output| {
            output.contains("\"status\":\"notRequiredSingleExecutorEquivalent\"")
        })
    {
        bail!("single-executor equivalent merge did not expose the review exemption");
    }
    if mode == ScriptMode::PostMergeImplementation
        && role == ScriptRole::Planner
        && step == 17
        && latest_function_call_output(request)
            .is_some_and(|output| output.contains("Tool execution error:"))
    {
        bail!("planner merge adjustment patch failed");
    }
    if mode == ScriptMode::PostMergeImplementation
        && role == ScriptRole::Planner
        && step == 18
        && !latest_function_call_output(request)
            .is_some_and(|output| output.contains("\"exitCode\":0"))
    {
        bail!("planner merge adjustment commit failed");
    }
    if mode == ScriptMode::PostMergeImplementation
        && role == ScriptRole::Planner
        && step == 20
        && !latest_function_call_output(request)
            .is_some_and(|output| output.contains("\"status\":\"required\""))
    {
        bail!("planner-adjusted merge did not require integrated review");
    }
    if mode == ScriptMode::PostMergeImplementation
        && role == ScriptRole::Planner
        && step == 23
        && !latest_function_call_output(request)
            .is_some_and(|output| output.contains("\"status\":\"satisfiedByReview\""))
    {
        bail!("integrated review did not satisfy the final completion gate");
    }
    if role == ScriptRole::Executor && step == 0 {
        let request_text = request.to_string();
        if !request_text.contains("design") || !request_text.contains("scopeHints") {
            bail!("executor instructions do not contain the normalized scopeHints focus");
        }
        if request_text.contains("ownedPaths") {
            bail!("executor instructions still expose legacy ownedPaths");
        }
    }
    if role == ScriptRole::Executor && step == 2 {
        let output = latest_function_call_output(request)
            .context("executor command failure did not return a tool result")?;
        if !output.contains("\"status\":\"failed\"") {
            bail!("fixture command was expected to fail before correction: {output}");
        }
    }
    if role == ScriptRole::Executor && step == 4 {
        let output = latest_function_call_output(request)
            .context("executor patch failure did not return a tool result")?;
        if !output.contains("Tool execution error:") {
            bail!("fixture patch was expected to fail before reread: {output}");
        }
    }
    if mode == ScriptMode::PostMergeImplementation && role == ScriptRole::Reviewer && step == 7 {
        let output = latest_function_call_output(request)
            .context("integrated reviewer exit did not return a tool result")?;
        bail!("integrated reviewer exit was rejected: {output}");
    }
    Ok(())
}

fn request_tools(request: &serde_json::Value) -> Result<&[serde_json::Value]> {
    request
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .context("model request has no tools array")
}

fn validate_planning_contract(tools: &[serde_json::Value]) -> Result<()> {
    let names = tools
        .iter()
        .filter_map(|tool| {
            tool.get("name")
                .or_else(|| {
                    tool.get("function")
                        .and_then(|function| function.get("name"))
                })
                .and_then(serde_json::Value::as_str)
        })
        .collect::<BTreeSet<_>>();
    for required in ["list_files", "read_file", "plan_exit"] {
        if !names.contains(required) {
            bail!("planning tools do not include {required}: {names:?}");
        }
    }
    for unavailable in [
        "exec",
        "write_stdin",
        "search_files",
        "task_status",
        "task_spawn_executor",
        "task_finalize_design",
        "task_record_merge",
        "task_request_delivery_review",
        "task_request_integrated_review",
        "read_review_round",
        "task_complete",
        "task_stop",
    ] {
        if names.contains(unavailable) {
            bail!("planning tools unexpectedly include {unavailable}: {names:?}");
        }
    }
    Ok(())
}

fn ensure_fresh_task_child(request: &serde_json::Value, role: &str) -> Result<()> {
    if request.to_string().contains(PARENT_HISTORY_MARKER) {
        bail!("{role} inherited the planner history marker");
    }
    Ok(())
}

fn validate_planner_spawn_contract(tools: &[serde_json::Value]) -> Result<()> {
    let executor = find_tool(tools, "task_spawn_executor")
        .context("planner tools do not include task_spawn_executor")?;
    let executor_schema = tool_parameters(executor)
        .context("task_spawn_executor does not expose function parameters")?;
    let required = executor_schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .context("task_spawn_executor schema has no required array")?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<BTreeSet<_>>();
    if required
        != BTreeSet::from([
            "acceptanceCriteria",
            "dependencies",
            "evidence",
            "implementationSteps",
            "objective",
            "scope",
            "taskName",
            "verification",
        ])
    {
        bail!("task_spawn_executor has unexpected required fields: {required:?}");
    }
    if executor_schema["properties"].get("message").is_some()
        || executor_schema["properties"].get("scopeHints").is_some()
        || executor_schema["properties"]
            .get("verificationCommands")
            .is_some()
    {
        bail!("task_spawn_executor still exposes legacy free-form fields");
    }
    if executor_schema["properties"].get("ownedPaths").is_some() {
        bail!("task_spawn_executor schema still exposes legacy ownedPaths");
    }

    let generic =
        find_tool(tools, "spawn_agent").context("planner tools do not include spawn_agent")?;
    let generic_schema =
        tool_parameters(generic).context("spawn_agent does not expose function parameters")?;
    if generic_schema["properties"]["role"]["enum"] != serde_json::json!(["explorer"]) {
        bail!("Task planner spawn_agent must only expose the explorer role");
    }
    Ok(())
}

fn find_tool<'a>(
    tools: &'a [serde_json::Value],
    expected_name: &str,
) -> Option<&'a serde_json::Value> {
    tools.iter().find(|tool| {
        tool.get("name")
            .or_else(|| {
                tool.get("function")
                    .and_then(|function| function.get("name"))
            })
            .and_then(serde_json::Value::as_str)
            == Some(expected_name)
    })
}

fn tool_parameters(tool: &serde_json::Value) -> Option<&serde_json::Value> {
    tool.get("parameters").or_else(|| {
        tool.get("function")
            .and_then(|function| function.get("parameters"))
    })
}

fn latest_function_call_output(request: &serde_json::Value) -> Option<&str> {
    request
        .get("input")?
        .as_array()?
        .iter()
        .rev()
        .find(|item| {
            item.get("type").and_then(serde_json::Value::as_str) == Some("function_call_output")
        })
        .and_then(|item| item.get("output"))
        .and_then(serde_json::Value::as_str)
}

async fn read_json_request(socket: &mut TcpStream) -> Result<serde_json::Value> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let (header_end, content_length) = loop {
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            bail!("model request closed before headers completed");
        }
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buffer[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .context("model request has no content-length")?;
            break (header_end, content_length);
        }
    };
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            bail!("model request closed before body completed");
        }
        buffer.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&buffer[body_start..body_start + content_length])
        .context("model request body is not JSON")
}

async fn write_response(
    socket: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await?;
    Ok(())
}

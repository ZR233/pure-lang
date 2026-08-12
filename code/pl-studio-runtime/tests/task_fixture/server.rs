use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{StudioRuntime, StudioTaskRuntime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use super::PARENT_HISTORY_MARKER;
use super::git::git_output;
use super::sse::{final_text, repeated_tool_call, tool_call};

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
enum ScriptRole {
    Planner,
    Executor,
    Reviewer,
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
    ) -> Self {
        let state = Arc::new(ScriptState {
            runtime,
            thread_id,
            workspace,
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
        let expected = (21, 11, 6);
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
        validate_request_step(&request, role, step)?;
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
        ScriptRole::Reviewer => reviewer_response(step),
    }
}

async fn planner_response(state: &ScriptState, step: usize) -> Result<(&'static str, String)> {
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
                    "content": "# Offline task plan\n\n1. Record the contract in `design/task-flow.md`.\n2. Implement in an executor worktree.\n3. Review the completion, merge it, then review the integrated Task HEAD."
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
            "task_spawn_executor(exact duplicate)",
            repeated_tool_call(
                "spawn-executor",
                "task_spawn_executor",
                serde_json::json!({
                    "taskName": "offline_executor",
                    "message": "Create src/feature.txt with the exact required content, commit it, verify it, and report the completion for review.",
                    "scopeHints": ["design"],
                    "verificationCommands": [{
                        "command": "git diff --check",
                        "cwd": ".",
                        "purpose": "verify the patch has no whitespace errors"
                    }]
                }),
            ),
        ),
        5..=8 => {
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
        9 => (
            "task_status(completion)",
            tool_call("status-completion", "task_status", serde_json::json!({})),
        ),
        10 => {
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
        14 => {
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
        15 => ("task_record_merge", {
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
            tool_call(
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
            )
        }),
        16 => (
            "task_update_design(consistency)",
            tool_call(
                "design-consistency",
                "task_update_design",
                serde_json::json!({"patch": CONSISTENCY_DESIGN_PATCH}),
            ),
        ),
        17 => (
            "task_request_integrated_review",
            tool_call(
                "request-integrated-review",
                "task_request_integrated_review",
                serde_json::json!({}),
            ),
        ),
        18 => (
            "list_agents(integrated-review)",
            tool_call(
                "list-integrated-review",
                "list_agents",
                serde_json::json!({}),
            ),
        ),
        19 => (
            "task_status(integrated-review)",
            tool_call(
                "status-integrated-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        20 => (
            "task_complete",
            tool_call("complete-task", "task_complete", serde_json::json!({})),
        ),
        _ => bail!("unexpected planner request step {step}"),
    };
    Ok(response)
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
        10 => {
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
                        "result": {
                            "kind": "delivery",
                            "headCommit": head,
                            "verificationSummary": "offline fixture content committed"
                        }
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
                "review-list-design",
                "list_files",
                serde_json::json!({"path": "design"}),
            ),
        ),
        1 => (
            "read_file(design)",
            tool_call(
                "review-read-design",
                "read_file",
                serde_json::json!({"path": "design/task-flow.md"}),
            ),
        ),
        2 => (
            "review_exit(pass)",
            tool_call(
                "review-pass",
                "review_exit",
                serde_json::json!({
                    "verdict": "pass",
                    "summary": "Implementation matches the reviewed offline task contract.",
                    "designReferences": [{
                        "path": "design/task-flow.md",
                        "section": "Offline Task Flow"
                    }],
                    "findings": []
                }),
            ),
        ),
        _ => bail!("unexpected reviewer request step {step}"),
    };
    Ok(response)
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
    if names.contains("plan_exit") || names.contains("task_update_design") {
        return Ok(ScriptRole::Planner);
    }
    bail!("cannot identify scripted role from tools: {names:?}")
}

fn validate_request_step(request: &serde_json::Value, role: ScriptRole, step: usize) -> Result<()> {
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
            .context("Planner rg --files did not return a tool result")?;
        if !output.contains("\"status\":\"completed\"") || !output.contains("\"exitCode\":0") {
            bail!("Planner rg --files was expected to succeed before plan_exit: {output}");
        }
    }
    if role == ScriptRole::Planner
        && step == 5
        && !latest_function_call_output(request)
            .is_some_and(|output| output.contains("\"reused\":true"))
    {
        bail!("the repeated executor allocation did not resolve to the durable WorkUnit");
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
    for required in ["exec", "write_stdin", "plan_exit"] {
        if !names.contains(required) {
            bail!("planning tools do not include {required}: {names:?}");
        }
    }
    for unavailable in [
        "search_files",
        "task_status",
        "task_spawn_executor",
        "task_update_design",
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
    if required != BTreeSet::from(["message", "taskName", "verificationCommands"]) {
        bail!("task_spawn_executor has unexpected required fields: {required:?}");
    }
    if executor_schema["properties"].get("scopeHints").is_none() {
        bail!("task_spawn_executor schema does not expose optional scopeHints");
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

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{StudioRuntime, StudioTaskRuntime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use super::PARENT_HISTORY_MARKER;
use super::git::git_output;
use super::sse::{final_text, tool_call};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptRole {
    Planner,
    Executor,
    Reviewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskFlowScenario {
    HappyPath,
    InterruptedExecutor,
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
    session_id: String,
    scenario: TaskFlowScenario,
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
        session_id: String,
        scenario: TaskFlowScenario,
    ) -> Self {
        let state = Arc::new(ScriptState {
            runtime,
            session_id,
            scenario,
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
        let expected = match self.state.scenario {
            TaskFlowScenario::HappyPath => (8, 5, 3),
            TaskFlowScenario::InterruptedExecutor => (10, 6, 3),
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
        Ok(())
    }

    pub(super) async fn wait_for_interrupted_executor_request(&self) -> Result<()> {
        if self.state.scenario != TaskFlowScenario::InterruptedExecutor {
            bail!("fixture is not configured for an interrupted executor");
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if self.state.progress.lock().await.executor > 0 {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!(
                    "executor did not enter the suspended first request\n{}",
                    self.diagnostics().await
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
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
        let (action, body) = scripted_response(&state, role, step).await?;
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
    if state.scenario == TaskFlowScenario::InterruptedExecutor {
        match step {
            4 => {
                let task = current_task(state).await?;
                let executor = task
                    .agents
                    .iter()
                    .find(|agent| agent.role == "executor" && agent.status == "running")
                    .context("running executor is absent from task projection")?;
                return Ok((
                    "send_input(interrupt)",
                    tool_call(
                        "interrupt-executor",
                        "send_input",
                        serde_json::json!({
                            "target": executor.agent_id,
                            "message": "Continue in the queued turn, commit the requested implementation, and finish with submit_delivery.",
                            "interrupt": true,
                        }),
                    ),
                ));
            }
            5 => {
                return Ok((
                    "final(interrupt-dispatched)",
                    final_text(
                        "interrupt-dispatched",
                        "The executor was interrupted and queued for continuation.",
                    ),
                ));
            }
            _ => {}
        }
    }
    let step = if state.scenario == TaskFlowScenario::InterruptedExecutor && step >= 6 {
        step - 2
    } else {
        step
    };
    let response = match step {
        0 => (
            "plan_exit",
            tool_call(
                "plan",
                "plan_exit",
                serde_json::json!({
                    "content": "# Offline task plan\n\n1. Record the design contract.\n2. Implement in an executor worktree.\n3. Merge, review, and complete the task."
                }),
            ),
        ),
        1 => (
            "final",
            final_text("plan-submitted", "Plan submitted for confirmation."),
        ),
        2 => (
            "task_update_design(initial)",
            tool_call(
                "design-initial",
                "task_update_design",
                serde_json::json!({"patch": INITIAL_DESIGN_PATCH}),
            ),
        ),
        3 => (
            "task_spawn_executor",
            tool_call(
                "spawn-executor",
                "task_spawn_executor",
                serde_json::json!({
                    "taskName": "offline_executor",
                    "message": "Create src/feature.txt with the exact required content, commit it, and submit the committed delivery.",
                    "ownedPaths": ["src/**"]
                }),
            ),
        ),
        4 => {
            let task = current_task(state).await?;
            let executor = task
                .agents
                .iter()
                .find(|agent| agent.role == "executor" && agent.head_commit.is_some())
                .context("delivered executor is absent from task projection")?;
            (
                "task_merge_agent",
                tool_call(
                    "merge-executor",
                    "task_merge_agent",
                    serde_json::json!({
                        "agentId": executor.agent_id,
                        "expectedHeadCommit": task.expected_head
                    }),
                ),
            )
        }
        5 => (
            "task_update_design(consistency)",
            tool_call(
                "design-consistency",
                "task_update_design",
                serde_json::json!({"patch": CONSISTENCY_DESIGN_PATCH}),
            ),
        ),
        6 => (
            "task_request_review",
            tool_call(
                "request-review",
                "task_request_review",
                serde_json::json!({}),
            ),
        ),
        7 => (
            "task_complete",
            tool_call("complete-task", "task_complete", serde_json::json!({})),
        ),
        _ => bail!("unexpected planner request step {step}"),
    };
    Ok(response)
}

async fn executor_response(state: &ScriptState, step: usize) -> Result<(&'static str, String)> {
    if state.scenario == TaskFlowScenario::InterruptedExecutor && step == 0 {
        return std::future::pending().await;
    }
    let step = if state.scenario == TaskFlowScenario::InterruptedExecutor {
        step - 1
    } else {
        step
    };
    let response = match step {
        0 => (
            "apply_patch",
            tool_call(
                "executor-patch",
                "apply_patch",
                serde_json::json!({"input": FEATURE_PATCH}),
            ),
        ),
        1 => (
            "exec(git add)",
            tool_call(
                "executor-add",
                "exec",
                serde_json::json!({"command": "git add -- src/feature.txt"}),
            ),
        ),
        2 => (
            "exec(git commit)",
            tool_call(
                "executor-commit",
                "exec",
                serde_json::json!({
                    "command": "git -c user.name=Pure -c user.email=pure@local commit -m \"test: add offline task fixture\""
                }),
            ),
        ),
        3 => {
            let task = current_task(state).await?;
            let work_unit = task
                .work_units
                .iter()
                .find(|unit| unit.agent_id.is_some())
                .context("executor work unit is absent from task projection")?;
            let head = git_output(Path::new(&work_unit.worktree_path), &["rev-parse", "HEAD"])?;
            (
                "submit_delivery",
                tool_call(
                    "executor-delivery",
                    "submit_delivery",
                    serde_json::json!({
                        "headCommit": head,
                        "verificationSummary": "offline fixture content committed"
                    }),
                ),
            )
        }
        4 => (
            "final",
            final_text("executor-complete", "Delivery submitted."),
        ),
        _ => bail!("unexpected executor request step {step}"),
    };
    Ok(response)
}

fn reviewer_response(step: usize) -> Result<(&'static str, String)> {
    let response = match step {
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
        .session_task_view(&state.session_id)
        .await?
        .context("task projection is not available")
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
    let tools = request
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .context("model request has no tools array")?;
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
    if names.contains("submit_delivery") {
        ensure_fresh_task_child(request, "executor")?;
        return Ok(ScriptRole::Executor);
    }
    if names.contains("review_exit") {
        ensure_fresh_task_child(request, "reviewer")?;
        return Ok(ScriptRole::Reviewer);
    }
    if names.contains("plan_exit") || names.contains("task_update_design") {
        validate_planner_spawn_contract(tools)?;
        return Ok(ScriptRole::Planner);
    }
    bail!("cannot identify scripted role from tools: {names:?}")
}

fn validate_request_step(request: &serde_json::Value, role: ScriptRole, step: usize) -> Result<()> {
    if role == ScriptRole::Executor && step == 0 {
        let request_text = request.to_string();
        if !request_text.contains("src/**") || !request_text.contains("ownedPaths") {
            bail!("executor instructions do not contain the normalized ownedPaths scope");
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
    if required != BTreeSet::from(["message", "ownedPaths", "taskName"]) {
        bail!("task_spawn_executor has unexpected required fields: {required:?}");
    }
    if executor_schema["properties"]["ownedPaths"]["minItems"] != 1 {
        bail!("task_spawn_executor ownedPaths must require at least one item");
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

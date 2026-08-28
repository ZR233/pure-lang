use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use pl_studio_runtime::{StudioReviewScope, StudioRuntime, StudioTaskRuntime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use super::PARENT_HISTORY_MARKER;
use super::git::git_output;
use super::sse::tool_call;

const INITIAL_DESIGN_PATCH: &str = r#"*** Begin Patch
*** Add File: design/task-flow.md
+# Offline Task Flow
+
+The implementation has two independent workstreams: create `src/feature.txt` containing exactly `offline integration verified`, and create `src/feature-two.txt` containing exactly `offline second workstream verified`; each value is followed by a newline.
*** End Patch"#;
const FEATURE_PATCH: &str = r#"*** Begin Patch
*** Add File: src/feature.txt
+offline integration verified
*** End Patch"#;
const SECOND_FEATURE_PATH: &str = "src/feature-two.txt";
const SECOND_FEATURE_PATCH: &str = r#"*** Begin Patch
*** Add File: src/feature-two.txt
+offline second workstream verified
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
    TwoExecutorIntegrated,
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
    wire_requests: Vec<CapturedWireRequest>,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
struct CapturedWireRequest {
    role: ScriptRole,
    step: usize,
    body: serde_json::Value,
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
            ScriptMode::SingleExecutorEquivalent => (23, 12, 3),
            ScriptMode::TwoExecutorIntegrated => (37, 24, 10),
        };
        if (progress.planner, progress.executor, progress.reviewer) != expected {
            let actual = (progress.planner, progress.executor, progress.reviewer);
            let requests = progress.requests.join("\n");
            drop(progress);
            let task = self
                .state
                .runtime
                .thread_task_view(&self.state.thread_id)
                .await?;
            bail!(
                "scripted model stopped at planner={}, executor={}, reviewer={}; expected {expected:?}\n{requests}\ntask projection:\n{task:#?}",
                actual.0,
                actual.1,
                actual.2,
            );
        }
        assert!(
            !progress
                .requests
                .iter()
                .any(|request| request.contains("list_agents(executor)")),
            "executor wait delta should not be followed by list_agents refresh"
        );
        if self.state.mode == ScriptMode::TwoExecutorIntegrated {
            let sequence = progress.requests.join("\n");
            for expected_action in [
                "task_transition(begin-integrated-review)",
                "wait_agents(integrated-review)",
                "task_status(integrated-review)",
                "task_transition(complete)",
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

    pub(super) async fn assert_wire_contract(&self, user_prompt_marker: &str) -> Result<()> {
        let progress = self.state.progress.lock().await;
        let request = |role, step| {
            progress
                .wire_requests
                .iter()
                .find(|request| request.role == role && request.step == step)
                .map(|request| &request.body)
                .with_context(|| format!("missing final wire request for {role:?}[{step}]"))
        };

        let planner = request(ScriptRole::Planner, 0)?;
        let planner_instructions = request_instructions(planner)?;
        if !planner_instructions.contains("你是 Pure-Lang 的工程协作代理") {
            bail!("planner final wire instructions are missing the base system prompt");
        }
        let planner_text = planner.to_string();
        for marker in [
            "GLOBAL_DEVELOPER_CONTEXT_MARKER",
            "GLOBAL_USER_CONTEXT_MARKER",
            "Task 模式由 root planner",
            "Offline acceptance project instructions",
            "offline-task-wire",
        ] {
            if !planner_text.contains(marker) {
                bail!("planner final wire body is missing prompt section `{marker}`");
            }
        }
        if !planner_text.contains(user_prompt_marker) {
            bail!("planner final wire body is missing the real user prompt marker");
        }
        validate_planning_contract(request_tools(planner)?)?;

        let planner_history = request(ScriptRole::Planner, 1)?;
        let planner_history_text = planner_history.to_string();
        if !planner_history_text.contains(user_prompt_marker)
            || !planner_history_text.contains("function_call_output")
        {
            bail!("planner continuation wire body is missing hot Turn history");
        }
        let planner_working = request(ScriptRole::Planner, 7)?;
        if !planner_working.to_string().contains("working") {
            bail!("planner working-phase wire body is missing Task phase context");
        }
        validate_planner_spawn_contract(request_tools(planner_working)?)?;

        let executor = request(ScriptRole::Executor, 0)?;
        let executor_text = executor.to_string();
        for marker in [
            "Task root planner 创建的 executor",
            "GLOBAL_DEVELOPER_CONTEXT_MARKER",
            "Offline acceptance project instructions",
            "offline-task-wire",
        ] {
            if !executor_text.contains(marker) {
                bail!("executor final wire body is missing prompt section `{marker}`");
            }
        }
        if !executor_text.contains("src/feature.txt") || executor_text.contains(user_prompt_marker)
        {
            bail!("executor wire body does not contain only its fresh durable handoff");
        }
        let executor_tools = request_tools(executor)?;
        if find_tool(executor_tools, "report_completion").is_none()
            || find_tool(executor_tools, "task_transition").is_some()
        {
            bail!("executor final wire tools violate the role boundary");
        }
        if self.state.mode == ScriptMode::TwoExecutorIntegrated {
            let second_executor = request(ScriptRole::Executor, 12)?;
            let second_executor_text = second_executor.to_string();
            if !second_executor_text.contains(SECOND_FEATURE_PATH)
                || second_executor_text == executor_text
            {
                bail!("second executor wire body does not contain its independent durable handoff");
            }
            let second_tools = request_tools(second_executor)?;
            if find_tool(second_tools, "report_completion").is_none()
                || find_tool(second_tools, "task_transition").is_some()
            {
                bail!("second executor final wire tools violate the role boundary");
            }
        }

        let delivery_reviewer = request(ScriptRole::Reviewer, 0)?;
        assert_reviewer_wire(delivery_reviewer, "delivery")?;
        if self.state.mode == ScriptMode::TwoExecutorIntegrated {
            let second_delivery_reviewer = request(ScriptRole::Reviewer, 3)?;
            assert_reviewer_wire(second_delivery_reviewer, "second delivery")?;
            let integrated_reviewer = request(ScriptRole::Reviewer, 6)?;
            assert_reviewer_wire(integrated_reviewer, "integrated")?;
            if !integrated_reviewer
                .to_string()
                .to_ascii_lowercase()
                .contains("integrat")
            {
                bail!("integrated reviewer wire body is missing the integrated handoff");
            }
        }
        Ok(())
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
        state
            .progress
            .lock()
            .await
            .wire_requests
            .push(CapturedWireRequest {
                role,
                step,
                body: request.clone(),
            });
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
            "task_status(planning)",
            tool_call("status-planning", "task_status", serde_json::json!({})),
        ),
        2 => (
            "task_transition(submit-plan)",
            transition_call(
                state,
                "submit-plan",
                "submitPlan",
                serde_json::json!({
                    "summary": "# Offline task plan\n\n1. Record the contract in `design/task-flow.md`.\n2. Implement in an executor worktree.\n3. Review the completion, merge it, then review the integrated Task HEAD."
                }),
            )
            .await?,
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
                state,
                "finish-document-editing",
                "finishDocumentEditing",
                serde_json::json!({"summary": "Recorded the executor contract in design/task-flow.md."}),
            )
            .await?,
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
            let executor_id = executor_agent_id(state).await?;
            let call_id = format!("wait-executor-{step}");
            (
                "wait_agents(executor)",
                tool_call(
                    &call_id,
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
        17 => {
            let task = current_task(state).await?;
            let work_unit = task
                .work_units
                .iter()
                .rev()
                .find(|unit| unit.agent_id.is_some())
                .context("executor work unit is absent before planner Git integration")?;
            let command = match state.mode {
                ScriptMode::SingleExecutorEquivalent => {
                    format!("git merge --ff-only {}", work_unit.branch)
                }
                ScriptMode::TwoExecutorIntegrated => format!(
                    "git -c user.name=\"Pure Studio\" -c user.email=pure-studio@local merge --no-ff {} -m \"test: integrate offline task fixture\"",
                    work_unit.branch
                ),
            };
            (
                "exec(planner git merge)",
                tool_call(
                    "merge-executor",
                    "exec",
                    serde_json::json!({"command": command}),
                ),
            )
        }
        18 if state.mode == ScriptMode::SingleExecutorEquivalent => {
            ("task_record_merge", record_merge_call(state).await?)
        }
        19 if state.mode == ScriptMode::SingleExecutorEquivalent => (
            "task_status(review-gate)",
            tool_call("status-review-gate", "task_status", serde_json::json!({})),
        ),
        20 if state.mode == ScriptMode::SingleExecutorEquivalent => (
            "update_todo_list(completed)",
            tool_call(
                "complete-planner-todo",
                "update_todo_list",
                serde_json::json!({
                    "explanation": "The reviewed delivery is merged and ready for Task completion.",
                    "items": [
                        {
                            "step": "Complete and review the implementation",
                            "status": "completed"
                        },
                        {
                            "step": "Merge the approved delivery",
                            "status": "completed"
                        }
                    ]
                }),
            ),
        ),
        21 if state.mode == ScriptMode::SingleExecutorEquivalent => (
            "task_status(completed-todo)",
            tool_call(
                "status-completed-todo",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        22 if state.mode == ScriptMode::SingleExecutorEquivalent => (
            "task_transition(complete)",
            transition_call(
                state,
                "complete-task",
                "complete",
                serde_json::json!({
                    "outcome": "succeeded",
                    "summary": "Offline Task fixture completed, reviewed, and integrated."
                }),
            )
            .await?,
        ),
        18 if state.mode == ScriptMode::TwoExecutorIntegrated => {
            ("task_record_merge(first)", record_merge_call(state).await?)
        }
        19 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "task_status(first-merge)",
            tool_call("status-first-merge", "task_status", serde_json::json!({})),
        ),
        20 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "task_spawn_executor(second)",
            tool_call(
                "spawn-second-executor",
                "task_spawn_executor",
                serde_json::json!({
                    "taskName": "offline_second_executor",
                    "objective": "Create and commit src/feature-two.txt with the exact second-workstream content.",
                    "scope": {
                        "inScope": ["Create and verify the independent second feature fixture."],
                        "outOfScope": ["Do not modify src/feature.txt, the design, or planner history."],
                        "scopeHints": ["src/feature-two.txt"]
                    },
                    "implementationSteps": [{
                        "id": "step-create-second",
                        "instruction": "Create src/feature-two.txt with the exact second-workstream content.",
                        "targets": [{"path": "src/feature-two.txt"}],
                        "expectedOutcome": "The second fixture file has the exact required content.",
                        "criterionIds": ["criterion-second-content"]
                    }, {
                        "id": "step-commit-second",
                        "instruction": "Commit and verify only the second fixture file.",
                        "targets": [{"path": "src/feature-two.txt"}],
                        "expectedOutcome": "The independent delivery is committed and clean.",
                        "criterionIds": ["criterion-second-commit"]
                    }],
                    "acceptanceCriteria": [{
                        "id": "criterion-second-content",
                        "requirement": "src/feature-two.txt has the exact second-workstream content."
                    }, {
                        "id": "criterion-second-commit",
                        "requirement": "The second delivery commit is clean and scope-isolated."
                    }],
                    "dependencies": [],
                    "evidence": [{
                        "path": "design/task-flow.md",
                        "symbol": "Offline Task Flow",
                        "note": "Confirmed two-workstream design contract."
                    }],
                    "verification": {
                        "commands": [{
                            "id": "check-second-diff",
                            "command": "git diff --check HEAD^ HEAD",
                            "cwd": ".",
                            "purpose": "Verify the second committed patch.",
                            "expectedOutcome": "The command exits successfully with no output.",
                            "criterionIds": ["criterion-second-commit"]
                        }],
                        "inspections": [{
                            "id": "inspect-second-feature",
                            "instruction": "Compare src/feature-two.txt with the confirmed design.",
                            "targets": [{"path": "src/feature-two.txt"}],
                            "expectedOutcome": "The second file matches exactly.",
                            "criterionIds": ["criterion-second-content"]
                        }]
                    }
                }),
            ),
        ),
        21..=24 if state.mode == ScriptMode::TwoExecutorIntegrated => {
            let executor_id = executor_agent_id(state).await?;
            let call_id = format!("wait-second-executor-{step}");
            (
                "wait_agents(second-executor)",
                tool_call(
                    &call_id,
                    "wait_agents",
                    serde_json::json!({"targets": [executor_id]}),
                ),
            )
        }
        25 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "task_status(second-completion)",
            tool_call(
                "status-second-completion",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        26 if state.mode == ScriptMode::TwoExecutorIntegrated => {
            let executor_id = executor_agent_id(state).await?;
            (
                "task_request_delivery_review(second)",
                tool_call(
                    "request-second-delivery-review",
                    "task_request_delivery_review",
                    serde_json::json!({"executorAgentId": executor_id}),
                ),
            )
        }
        27 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "list_agents(second-delivery-review)",
            tool_call(
                "list-second-delivery-review",
                "list_agents",
                serde_json::json!({}),
            ),
        ),
        28 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "task_status(second-delivery-review)",
            tool_call(
                "status-second-delivery-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        29 if state.mode == ScriptMode::TwoExecutorIntegrated => {
            let executor_id = executor_agent_id(state).await?;
            (
                "close_agent(second-executor)",
                tool_call(
                    "close-second-executor",
                    "close_agent",
                    serde_json::json!({"target": executor_id}),
                ),
            )
        }
        30 if state.mode == ScriptMode::TwoExecutorIntegrated => {
            let task = current_task(state).await?;
            let work_unit = task
                .work_units
                .iter()
                .rev()
                .find(|unit| unit.agent_id.is_some())
                .context("second executor work unit is absent before Git integration")?;
            (
                "exec(planner second git merge)",
                tool_call(
                    "merge-second-executor",
                    "exec",
                    serde_json::json!({
                        "command": format!(
                            "git -c user.name=\"Pure Studio\" -c user.email=pure-studio@local merge --no-ff {} -m \"test: integrate second offline workstream\"",
                            work_unit.branch
                        )
                    }),
                ),
            )
        }
        31 if state.mode == ScriptMode::TwoExecutorIntegrated => {
            ("task_record_merge(second)", record_merge_call(state).await?)
        }
        32 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "task_status(review-gate)",
            tool_call("status-review-gate", "task_status", serde_json::json!({})),
        ),
        33 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "task_transition(begin-integrated-review)",
            transition_call(
                state,
                "begin-integrated-review",
                "beginIntegratedReview",
                serde_json::json!({}),
            )
            .await?,
        ),
        34 if state.mode == ScriptMode::TwoExecutorIntegrated => {
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
        35 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "task_status(integrated-review)",
            tool_call(
                "status-integrated-review",
                "task_status",
                serde_json::json!({}),
            ),
        ),
        36 if state.mode == ScriptMode::TwoExecutorIntegrated => (
            "task_transition(complete)",
            transition_call(
                state,
                "complete-task",
                "complete",
                serde_json::json!({
                    "outcome": "succeeded",
                    "summary": "Both offline workstreams completed after integrated review."
                }),
            )
            .await?,
        ),
        _ => bail!("unexpected planner request step {step}"),
    };
    Ok(response)
}

async fn transition_call(
    state: &ScriptState,
    call_id: &str,
    action: &str,
    fields: serde_json::Value,
) -> Result<String> {
    let task = current_task(state).await?;
    let mut input = fields
        .as_object()
        .cloned()
        .context("task_transition fixture fields must be an object")?;
    input.insert("action".to_string(), serde_json::json!(action));
    input.insert(
        "expectedRevision".to_string(),
        serde_json::json!(task.revision),
    );
    input.insert(
        "expectedGeneration".to_string(),
        serde_json::json!(task.generation),
    );
    Ok(tool_call(
        call_id,
        "task_transition",
        serde_json::Value::Object(input),
    ))
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
    if resulting_head == completion.base_commit {
        bail!("planner Git merge did not advance the main workspace HEAD");
    }
    Ok(tool_call(
        "record-executor-merge",
        "task_record_merge",
        serde_json::json!({
            "executorAgentId": executor_id,
            "completionRevision": completion.revision,
            "expectedPreviousHead": completion.base_commit,
            "resultingHead": resulting_head,
            "method": "merge",
            "summary": "Planner merged the approved offline executor branch with ordinary Git."
        }),
    ))
}

async fn executor_response(state: &ScriptState, step: usize) -> Result<(&'static str, String)> {
    let second = step >= 12;
    let step = step % 12;
    let feature_path = if second {
        SECOND_FEATURE_PATH
    } else {
        "src/feature.txt"
    };
    let feature_patch = if second {
        SECOND_FEATURE_PATCH
    } else {
        FEATURE_PATCH
    };
    let check_id = if second {
        "check-second-diff"
    } else {
        "check-diff"
    };
    let inspection_id = if second {
        "inspect-second-feature"
    } else {
        "inspect-feature"
    };
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
                serde_json::json!({"input": feature_patch}),
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
                serde_json::json!({"command": format!("git add -- {feature_path}")}),
            ),
        ),
        8 => (
            "exec(git commit)",
            tool_call(
                "executor-commit",
                "exec",
                serde_json::json!({
                    "command": format!(
                        "git -c user.name=Pure -c user.email=pure@local commit -m \"test: add {} offline task fixture\"",
                        if second { "second" } else { "first" }
                    )
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
                .rev()
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
                        "changedFiles": [feature_path],
                        "verificationResults": [{
                            "checkId": check_id,
                            "summary": "git diff --check HEAD^ HEAD exited successfully with no output"
                        }, {
                            "checkId": inspection_id,
                            "summary": format!("{feature_path} matches the confirmed fixture content exactly")
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
        (_, 2) => (
            "review_exit(first-pass)",
            review_exit_call(false, "src/feature.txt"),
        ),
        (ScriptMode::TwoExecutorIntegrated, 3) => (
            "list_files(second-design)",
            tool_call(
                "second-review-list-design",
                "list_files",
                serde_json::json!({"path": "design"}),
            ),
        ),
        (ScriptMode::TwoExecutorIntegrated, 4) => (
            "read_file(second-design)",
            tool_call(
                "second-review-read-design",
                "read_file",
                serde_json::json!({"path": "design/task-flow.md"}),
            ),
        ),
        (ScriptMode::TwoExecutorIntegrated, 5) => (
            "review_exit(second-pass)",
            review_exit_call(false, SECOND_FEATURE_PATH),
        ),
        (ScriptMode::TwoExecutorIntegrated, 6) => (
            "list_files(integrated)",
            tool_call(
                "integrated-list-files",
                "list_files",
                serde_json::json!({"path": "src"}),
            ),
        ),
        (ScriptMode::TwoExecutorIntegrated, 7) => (
            "read_file(integrated-design)",
            tool_call(
                "integrated-read-design",
                "read_file",
                serde_json::json!({"path": "design/task-flow.md"}),
            ),
        ),
        (ScriptMode::TwoExecutorIntegrated, 8) => (
            "read_file(integrated-change)",
            tool_call(
                "integrated-read-change",
                "read_file",
                serde_json::json!({"path": SECOND_FEATURE_PATH}),
            ),
        ),
        (ScriptMode::TwoExecutorIntegrated, 9) => (
            "review_exit(integrated-pass)",
            review_exit_call(true, SECOND_FEATURE_PATH),
        ),
        _ => bail!("unexpected reviewer request step {step}"),
    };
    Ok(response)
}

fn review_exit_call(integrated: bool, feature_path: &str) -> String {
    let file_reviews = if integrated {
        serde_json::json!([{
            "path": "src/feature.txt",
            "reviewed": true
        }, {
            "path": SECOND_FEATURE_PATH,
            "reviewed": true
        }])
    } else {
        serde_json::json!([{
            "path": feature_path,
            "reviewed": true
        }])
    };
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
            "fileReviews": file_reviews
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
        .rev()
        .find_map(|unit| unit.agent_id)
        .context("executor is absent from task projection")
}

async fn integrated_reviewer_agent_id(state: &ScriptState) -> Result<String> {
    current_task(state)
        .await?
        .reviews
        .into_iter()
        .filter(|review| review.scope == StudioReviewScope::Integrated)
        .find_map(|review| review.state.reviewer_agent_id().map(str::to_string))
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
    if names.contains("task_status") || names.contains("task_transition") {
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
                "Planner list_files was expected to observe README.md before task_status: {output}"
            );
        }
    }
    if role == ScriptRole::Planner && step == 2 {
        let output = latest_function_call_output(request)
            .context("task_status did not return a tool output")?;
        let output: serde_json::Value =
            serde_json::from_str(output).context("task_status output is not JSON")?;
        if output["task"]["state"]["kind"] != "planning" || output["task"]["generation"] != 0 {
            bail!("task_status did not expose the canonical Planning state");
        }
    }
    if mode == ScriptMode::SingleExecutorEquivalent && role == ScriptRole::Planner && step == 20 {
        let output = latest_function_call_output(request)
            .context("single-executor review-gate status returned no tool output")?;
        if !output.contains("\"status\":\"notRequiredSingleExecutorEquivalent\"") {
            bail!("single-executor equivalent merge did not expose the review exemption: {output}");
        }
    }
    if mode == ScriptMode::SingleExecutorEquivalent && role == ScriptRole::Planner && step == 22 {
        let output = latest_function_call_output(request)
            .context("completed-todo status returned no tool output")?;
        if !output.contains("\"completionGate\":{\"available\":true")
            || !output.contains("\"status\":\"completed\"")
            || output.contains("待办尚未完成")
        {
            bail!("completed todo did not leave the completion gate available: {output}");
        }
    }
    if mode == ScriptMode::TwoExecutorIntegrated && role == ScriptRole::Planner && step == 33 {
        let output = latest_function_call_output(request)
            .context("two-executor review gate returned no tool output")?;
        let parsed: serde_json::Value =
            serde_json::from_str(output).context("planner review-gate output is not JSON")?;
        if parsed["completionGate"]["reviewGate"]["status"] != "required" {
            bail!("two-executor merge did not require integrated review: {output}");
        }
    }
    if mode == ScriptMode::TwoExecutorIntegrated && role == ScriptRole::Planner && step == 36 {
        let output = latest_function_call_output(request)
            .context("integrated review status returned no tool output")?;
        let parsed: serde_json::Value =
            serde_json::from_str(output).context("integrated review status output is not JSON")?;
        if parsed["completionGate"]["reviewGate"]["status"] != "satisfiedByReview" {
            bail!("integrated review did not satisfy the final completion gate: {output}");
        }
    }
    let executor_step = step % 12;
    if role == ScriptRole::Executor && executor_step == 0 {
        let request_text = request.to_string();
        if !request_text.contains("design") || !request_text.contains("scopeHints") {
            bail!("executor instructions do not contain the normalized scopeHints focus");
        }
        if request_text.contains("ownedPaths") {
            bail!("executor instructions still expose legacy ownedPaths");
        }
    }
    if role == ScriptRole::Executor && executor_step == 2 {
        let output = latest_function_call_output(request)
            .context("executor command failure did not return a tool result")?;
        if !output.contains("\"kind\":\"failed\"") {
            bail!("fixture command was expected to fail before correction: {output}");
        }
    }
    if role == ScriptRole::Executor && executor_step == 4 {
        let output = latest_function_call_output(request)
            .context("executor patch failure did not return a tool result")?;
        if !output.contains("Tool execution error:") {
            bail!("fixture patch was expected to fail before reread: {output}");
        }
    }
    if mode == ScriptMode::TwoExecutorIntegrated && role == ScriptRole::Reviewer && step == 10 {
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

fn request_instructions(request: &serde_json::Value) -> Result<&str> {
    request
        .get("instructions")
        .and_then(serde_json::Value::as_str)
        .context("final wire request has no instructions string")
}

fn assert_reviewer_wire(request: &serde_json::Value, scope: &str) -> Result<()> {
    let request_text = request.to_string();
    for marker in [
        "Task runtime 创建的 reviewer",
        "不得修改被审查现场",
        "GLOBAL_DEVELOPER_CONTEXT_MARKER",
        "Offline acceptance project instructions",
        "offline-task-wire",
    ] {
        if !request_text.contains(marker) {
            bail!("{scope} reviewer final wire body is missing prompt section `{marker}`");
        }
    }
    let tools = request_tools(request)?;
    let tool_names = tools
        .iter()
        .filter_map(|tool| {
            tool.get("name")
                .or_else(|| tool.get("function").and_then(|value| value.get("name")))
                .and_then(serde_json::Value::as_str)
        })
        .collect::<BTreeSet<_>>();
    if find_tool(tools, "review_exit").is_none()
        || find_tool(tools, "apply_patch").is_some()
        || find_tool(tools, "task_transition").is_some()
    {
        bail!(
            "{scope} reviewer final wire tools violate the read-only role boundary: {tool_names:?}"
        );
    }
    if !request_text.contains("design/task-flow.md") {
        bail!("{scope} reviewer final wire body is missing its review handoff");
    }
    Ok(())
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
    for required in [
        "list_files",
        "read_file",
        "exec",
        "task_status",
        "task_transition",
    ] {
        if !names.contains(required) {
            bail!("planning tools do not include {required}: {names:?}");
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

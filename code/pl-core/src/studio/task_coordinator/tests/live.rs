use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_protocol::{InteractionPayload, InteractionResolution, ToolApprovalResolution};
use pl_trace::{TraceEvent, TraceEventKind};
use tokio::time::{Duration, timeout};

use super::*;
use crate::{CoreSession, PureCore, TurnRequest, TurnResult, TurnResultStatus};

const DEEPSEEK_LIVE_ENV_KEY: &str = "API_KEY_DEEPSEEK";

struct LiveWorkspace {
    path: PathBuf,
}

impl LiveWorkspace {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!(
                "pure-task-orchestration-live-{}-{stamp}",
                std::process::id()
            )),
        }
    }
}

impl Drop for LiveWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn live_api_key() -> Option<String> {
    match std::env::var(DEEPSEEK_LIVE_ENV_KEY) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("{DEEPSEEK_LIVE_ENV_KEY} is not set; skipping live Task orchestration test");
            None
        }
    }
}

fn approval_options(requested_tools: Arc<Mutex<Vec<String>>>) -> TurnOptions {
    TurnOptions::default().with_interaction_callback(Arc::new(move |interaction| {
        let requested_tools = requested_tools.clone();
        Box::pin(async move {
            let InteractionPayload::ToolApproval { name, .. } = interaction.payload else {
                return InteractionResolution::ToolApproval {
                    decision: ToolApprovalResolution::Denied,
                    reason: Some("unexpected interaction payload".to_string()),
                };
            };
            requested_tools.lock().unwrap().push(name.clone());
            let allowed = matches!(
                name.as_str(),
                "task_update_design"
                    | "spawn_agent"
                    | "wait_agent"
                    | "list_agents"
                    | "task_merge_agent"
                    | "task_request_review"
                    | "task_complete"
                    | "read_file"
                    | "list_files"
                    | "search_files"
                    | "stat_path"
                    | "apply_patch"
                    | "bash"
                    | "submit_delivery"
                    | "review_exit"
            );
            InteractionResolution::ToolApproval {
                decision: if allowed {
                    ToolApprovalResolution::Approved
                } else {
                    ToolApprovalResolution::Denied
                },
                reason: (!allowed).then(|| format!("tool is outside live test scope: {name}")),
            }
        })
    }))
}

async fn run_live_turn(
    core: &PureCore,
    session: &mut CoreSession,
    prompt: &str,
    options: TurnOptions,
) -> TurnResult {
    let turn_id = format!("live-{}", session.len());
    let (event_tx, _) = tokio::sync::broadcast::channel(256);
    let mut recorder = crate::TraceRecorder::new(turn_id.clone(), event_tx, 0);
    let request = TurnRequest::new(prompt, CompileMode::Task).with_budget(TurnBudget::new(240_000));
    timeout(
        Duration::from_secs(300),
        core.run_turn_with_trace(session, request, &mut recorder, options),
    )
    .await
    .expect("live model turn timed out")
    .expect("live model turn failed to execute")
}

fn assert_completed_with_tool(result: &TurnResult, tool_name: &str) {
    let diagnostics = tool_diagnostics(&result.trace_events);
    assert_eq!(
        result.status,
        TurnResultStatus::Completed,
        "live turn failed: {:?}\n{diagnostics}",
        result.error
    );
    assert!(
        result.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.tool.as_ref().is_some_and(|tool| tool.name == tool_name)
        )),
        "live turn did not complete `{tool_name}`\n{diagnostics}"
    );
}

fn tool_diagnostics(events: &[TraceEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartStarted { item } => item.tool.as_ref().map(|tool| {
                format!(
                    "{} {:?} args={} result={}",
                    tool.name,
                    item.status,
                    tool.arguments,
                    tool.result.as_deref().unwrap_or("")
                )
            }),
            TraceEventKind::TracePartFailed { item, error } => item.tool.as_ref().map(|tool| {
                format!(
                    "{} {:?} args={} error={error}",
                    tool.name, item.status, tool.arguments
                )
            }),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn git_output(repository: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

async fn wait_for_delivery(store: &StudioStore, run_id: &str) -> AgentOutcomeRecord {
    timeout(Duration::from_secs(300), async {
        loop {
            if let Some(outcome) = store
                .list_agent_outcomes(run_id)
                .await
                .unwrap()
                .into_iter()
                .find(|outcome| outcome.role == "executor" && outcome.delivery.is_some())
            {
                break outcome;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("executor did not submit delivery")
}

enum LiveReviewOutcome {
    Pass,
    RetryableFailure(String),
    ReviewVerdict(String),
}

async fn wait_for_review_pass(
    store: &StudioStore,
    coordinator: &TaskCoordinator,
    supervisor: &AgentSupervisor,
    studio_session_id: &str,
    run_id: &str,
) -> Result<LiveReviewOutcome, String> {
    timeout(Duration::from_secs(300), async {
        loop {
            let rounds = store.list_review_rounds(run_id).await.unwrap();
            let latest_round = rounds.last();
            if latest_round.is_some_and(|round| round.verdict == ReviewVerdict::Pass) {
                break Ok(LiveReviewOutcome::Pass);
            }
            if let Some(round) = latest_round
                && round.verdict != ReviewVerdict::Pending
            {
                let error = format!(
                    "reviewer returned {:?}: summary={:?}, findings={:?}",
                    round.verdict, round.summary, round.findings
                );
                break Ok(if round.verdict == ReviewVerdict::Failed {
                    LiveReviewOutcome::RetryableFailure(error)
                } else {
                    LiveReviewOutcome::ReviewVerdict(error)
                });
            }
            let latest_reviewer_id = latest_round
                .and_then(|round| round.reviewer_agent_id.as_deref());
            if let Some(outcome) = store
                .list_agent_outcomes(run_id)
                .await
                .unwrap()
                .into_iter()
                .find(|outcome| {
                    outcome.role == "reviewer"
                        && latest_reviewer_id == Some(outcome.agent_id.as_str())
                })
                && let Some(record) = supervisor.record(&outcome.agent_id).await
                && record.status.is_final()
            {
                let session = supervisor.load_session(&outcome.agent_id).await;
                let diagnostics = session
                    .as_ref()
                    .map(reviewer_session_diagnostics)
                    .unwrap_or_else(|| "reviewer session unavailable".to_string());
                let recording = coordinator
                    .record_terminal_agent_state(
                        studio_session_id,
                        &crate::agent::AgentTerminalStateChange {
                            agent_id: outcome.agent_id,
                            role: "reviewer".to_string(),
                            status: record.status,
                            summary: record.summary.clone(),
                            error: record.error.clone(),
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                if !matches!(recording, TerminalAgentStateRecording::Changed { .. }) {
                    let run = store.read_task_run(run_id).await.unwrap().unwrap();
                    break Err(format!(
                        "reviewer terminal state was not persisted: recording={recording:?}, phase={:?}",
                        run.phase
                    ));
                }
                let run = store.read_task_run(run_id).await.unwrap().unwrap();
                let latest = store
                    .list_review_rounds(run_id)
                    .await
                    .unwrap()
                    .pop()
                    .unwrap();
                if latest.verdict == ReviewVerdict::Pass {
                    break Ok(LiveReviewOutcome::Pass);
                }
                if !matches!(
                    latest.verdict,
                    ReviewVerdict::Pending | ReviewVerdict::Failed
                ) {
                    break Ok(LiveReviewOutcome::ReviewVerdict(format!(
                        "reviewer returned {:?}: summary={:?}, findings={:?}",
                        latest.verdict, latest.summary, latest.findings
                    )));
                }
                if !matches!(
                    run.phase,
                    TaskRunPhase::Implementing | TaskRunPhase::Reworking
                ) {
                    break Err(format!(
                        "reviewer failure did not restore a reviewable phase: {:?}, round={:?}",
                        run.phase, latest.verdict
                    ));
                }
                break Ok(LiveReviewOutcome::RetryableFailure(format!(
                    "reviewer ended without pass: status={:?}, summary={:?}, error={:?}\n{}",
                    record.status, record.summary, record.error, diagnostics
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| "reviewer did not return pass before timeout".to_string())?
}

fn reviewer_session_diagnostics(session: &CoreSession) -> String {
    session
        .messages()
        .iter()
        .filter_map(|message| {
            let metadata =
                pl_protocol::ToolResultMetadata::from_metadata(&message.metadata).ok()?;
            Some(format!(
                "{} args={} result={}",
                metadata.tool_name,
                metadata.tool_call_arguments.as_deref().unwrap_or(""),
                crate::message_content_text(&message.content)
            ))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
#[ignore = "requires explicit live DeepSeek credentials and incurs model usage"]
async fn live_deepseek_completes_task_orchestration_with_worktree_merge_and_review() {
    let Some(api_key) = live_api_key() else {
        return;
    };
    let workspace = LiveWorkspace::new();
    std::fs::create_dir_all(&workspace.path).unwrap();
    std::fs::write(workspace.path.join("README.md"), "# Live Task\n").unwrap();

    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace.path).await.unwrap();
    let studio_session = store
        .create_session(&project.id, "Live Task", CompileMode::Task)
        .await
        .unwrap();
    let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
    let run = coordinator
        .start_confirmed_task(
            &studio_session.id,
            "Create src/feature.txt with the exact text verified by live task test.",
            &workspace.path,
        )
        .await
        .unwrap();
    assert!(workspace.path.join(".git").exists());
    let baseline_head = git_output(&workspace.path, &["rev-parse", "HEAD"]);

    let mut provider_info = pl_model::ProviderInfo::deepseek(None);
    provider_info.bearer_token = Some(api_key);
    let supervisor = AgentSupervisor::default();
    supervisor
        .enable_worktrees(PathBuf::from(&run.workspace_root))
        .await;
    let mut core = PureCoreBuilder::from_provider_info(provider_info)
        .unwrap()
        .with_agent_supervisor(supervisor.clone())
        .build();
    core.register_default_tools(workspace.path.clone(), None)
        .await;
    coordinator.install_tools(&mut core, &studio_session.id);
    let requested_tools = Arc::new(Mutex::new(Vec::new()));
    let options = approval_options(requested_tools.clone());
    let mut planner_session = CoreSession::new();

    let design = run_live_turn(
        &core,
        &mut planner_session,
        r#"Call task_update_design exactly once with this patch. After it succeeds, return a final response immediately. Do not call task_stop or any other tool:
*** Begin Patch
*** Add File: design/live-task.md
+# Live Task Contract
+
+The implementation must create `src/feature.txt` containing exactly `verified by live task test` followed by a newline.
*** End Patch"#,
        options.clone(),
    )
    .await;
    assert_completed_with_tool(&design, "task_update_design");
    let implementing = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(
        implementing.phase,
        TaskRunPhase::Implementing,
        "unexpected phase after design: {:?}\n{}",
        implementing.status_message,
        tool_diagnostics(&design.trace_events)
    );

    let spawn = run_live_turn(
        &core,
        &mut planner_session,
        r#"Call spawn_agent exactly once with agentType `executor`, taskName `live_executor`, ownedPaths [`src/**`], and the message below. After spawn_agent succeeds, return a final response immediately; do not call task_stop or any other planner tool.

Executor message:
Create src/feature.txt containing exactly `verified by live task test` plus a trailing newline. Use apply_patch. Then run `git add -- src/feature.txt` and commit it with git using temporary `-c user.name=Pure -c user.email=pure@local` identity. Read HEAD with `git rev-parse HEAD` and call submit_delivery with that exact commit and verificationSummary `content verified`. Do not finish without submit_delivery."#,
        options.clone(),
    )
    .await;
    assert_completed_with_tool(&spawn, "spawn_agent");
    let delivery = wait_for_delivery(&store, &run.id).await;
    let delivery_commit = delivery.delivery.as_ref().unwrap().head_commit.clone();
    assert_eq!(
        git_output(&workspace.path, &["rev-parse", "HEAD"]),
        implementing.expected_head
    );
    assert_ne!(delivery_commit, implementing.expected_head);

    let merge_prompt = format!(
        "Call task_merge_agent exactly once with agentId `{}` and expectedHeadCommit `{}`. After it succeeds, return a final response immediately; do not call task_stop or any other tool.",
        delivery.agent_id, implementing.expected_head
    );
    let merge = run_live_turn(&core, &mut planner_session, &merge_prompt, options.clone()).await;
    assert_completed_with_tool(&merge, "task_merge_agent");
    let merged = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_ne!(merged.expected_head, implementing.expected_head);
    assert_eq!(
        std::fs::read_to_string(workspace.path.join("src/feature.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "verified by live task test\n"
    );

    let design_consistency = run_live_turn(
        &core,
        &mut planner_session,
        r#"Call task_update_design exactly once with this implementation-consistency patch. After it succeeds, return a final response immediately; do not call task_stop or any other tool:
*** Begin Patch
*** Update File: design/live-task.md
@@
 The implementation must create `src/feature.txt` containing exactly `verified by live task test` followed by a newline.
+
+Implementation status: completed and merged.
*** End Patch"#,
        options.clone(),
    )
    .await;
    assert_completed_with_tool(&design_consistency, "task_update_design");

    let mut review_failures = Vec::new();
    let mut review_passed = false;
    for _ in 0..3 {
        let review = run_live_turn(
            &core,
            &mut planner_session,
            "Call task_request_review exactly once with no arguments. After it succeeds, return a final response immediately; do not call task_stop or any other tool.",
            options.clone(),
        )
        .await;
        assert_completed_with_tool(&review, "task_request_review");
        match wait_for_review_pass(
            &store,
            &coordinator,
            &supervisor,
            &studio_session.id,
            &run.id,
        )
        .await
        {
            Ok(LiveReviewOutcome::Pass) => {
                review_passed = true;
                break;
            }
            Ok(LiveReviewOutcome::RetryableFailure(error)) => {
                let phase = store.read_task_run(&run.id).await.unwrap().unwrap().phase;
                assert!(
                    matches!(phase, TaskRunPhase::Implementing | TaskRunPhase::Reworking),
                    "retryable reviewer failure left task in {phase:?}: {error}"
                );
                review_failures.push(error);
            }
            Ok(LiveReviewOutcome::ReviewVerdict(error)) => {
                panic!("live reviewer requested task changes: {error}")
            }
            Err(error) => panic!("live reviewer wait failed: {error}"),
        }
    }
    assert!(
        review_passed,
        "review did not pass after three rounds:\n{}",
        review_failures.join("\n---\n")
    );
    let reviewer = store
        .list_agent_outcomes(&run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.role == "reviewer" && outcome.review.is_some())
        .expect("reviewer outcome");
    assert!(!reviewer.requested_by_call_id.is_empty());
    let review = reviewer.review.expect("review result");
    assert!(
        review
            .design_references
            .iter()
            .any(|reference| reference.path == "design/live-task.md")
    );

    let complete = run_live_turn(
        &core,
        &mut planner_session,
        "Call task_complete exactly once with no arguments. After it succeeds, return a final response immediately; do not call task_stop or any other tool.",
        options,
    )
    .await;
    assert_completed_with_tool(&complete, "task_complete");
    let completed = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(completed.phase, TaskRunPhase::Completed);
    assert!(git_output(&workspace.path, &["status", "--porcelain"]).is_empty());
    assert!(
        git_output(
            &workspace.path,
            &["merge-base", "--is-ancestor", &delivery_commit, "HEAD"]
        )
        .is_empty()
    );
    assert!(
        store
            .list_work_units(&run.id)
            .await
            .unwrap()
            .iter()
            .all(|unit| unit.status == WorkUnitStatus::Merged)
    );
    assert!(
        store
            .list_work_units(&run.id)
            .await
            .unwrap()
            .iter()
            .all(|unit| !Path::new(&unit.worktree_path).exists())
    );
    assert_ne!(baseline_head, completed.expected_head);
    assert!(
        requested_tools
            .lock()
            .unwrap()
            .contains(&"bash".to_string())
    );
}

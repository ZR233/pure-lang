use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use pl_protocol::{AgentStatus, PureError};
use serde_json::json;
use tokio::sync::{Mutex, oneshot};
use tokio::time::{Duration, timeout};
use tokio_util::sync::CancellationToken;

use super::state::AgentEntry;
use super::*;
use crate::agent::worktree::{
    MergeOutcome, WorktreeBackend, WorktreeCreateFailure, WorktreeCreateSpec, WorktreeError,
    WorktreeManager,
};
use crate::turn::{CompileMode, TurnBudget, TurnOptions};

#[derive(Debug)]
struct RecordingSpawnLifecycleHook {
    calls: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug)]
struct FailingRollbackLifecycleHook {
    calls: Arc<Mutex<Vec<String>>>,
    worktree: WorktreeCreateSpec,
}

impl AgentLifecycleHook for FailingRollbackLifecycleHook {
    fn prepare_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<AgentSpawnPreparation, PureError>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.calls
                .lock()
                .await
                .push(format!("prepare:{}", request.agent_id));
            Ok(AgentSpawnPreparation::with_worktree(self.worktree.clone()))
        })
    }

    fn activate_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
        _preparation: &'a AgentSpawnPreparation,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls
                .lock()
                .await
                .push(format!("activate:{}", request.agent_id));
            Ok(())
        })
    }

    fn rollback_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
        _preparation: &'a AgentSpawnPreparation,
        _error: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls
                .lock()
                .await
                .push(format!("hook_rollback:{}", request.agent_id));
            Err(PureError::ToolExecutionFailed {
                tool: "task_coordinator".to_string(),
                error: "hook rollback failed".to_string(),
            })
        })
    }

    fn validate_close<'a>(
        &'a self,
        _request: &'a AgentCloseLifecycleRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug)]
struct FailingSupervisorWorktreeBackend {
    calls: Arc<Mutex<Vec<String>>>,
}

impl WorktreeBackend for FailingSupervisorWorktreeBackend {
    fn create<'a>(
        &'a self,
        _repo_root: &'a std::path::Path,
        _branch: &'a str,
        _target_path: &'a std::path::Path,
        _base_commit: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), WorktreeCreateFailure>> + Send + 'a>>
    {
        Box::pin(async move {
            self.calls.lock().await.push("create".to_string());
            Ok(())
        })
    }

    fn remove<'a>(
        &'a self,
        _repo_root: &'a std::path::Path,
        _target_path: &'a std::path::Path,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), WorktreeError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.lock().await.push(format!("remove:{force}"));
            Err(supervisor_git_error("remove cleanup failed"))
        })
    }

    fn delete_branch<'a>(
        &'a self,
        _repo_root: &'a std::path::Path,
        _branch: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), WorktreeError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.lock().await.push("delete_branch".to_string());
            Err(supervisor_git_error("branch cleanup failed"))
        })
    }

    fn commit_all<'a>(
        &'a self,
        _worktree_path: &'a std::path::Path,
        _message: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), WorktreeError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn merge_branch<'a>(
        &'a self,
        _main_workspace: &'a std::path::Path,
        _branch: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<MergeOutcome, WorktreeError>> + Send + 'a>>
    {
        Box::pin(async { Ok(MergeOutcome::Merged) })
    }
}

fn supervisor_git_error(stderr: &str) -> WorktreeError {
    WorktreeError::GitCommand {
        args: "cleanup".to_string(),
        stderr: stderr.to_string(),
    }
}

impl AgentLifecycleHook for RecordingSpawnLifecycleHook {
    fn prepare_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<AgentSpawnPreparation, PureError>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.calls
                .lock()
                .await
                .push(format!("prepare:{}", request.agent_id));
            Ok(AgentSpawnPreparation::without_worktree())
        })
    }

    fn activate_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
        _preparation: &'a AgentSpawnPreparation,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls
                .lock()
                .await
                .push(format!("activate:{}", request.agent_id));
            Ok(())
        })
    }

    fn rollback_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
        _preparation: &'a AgentSpawnPreparation,
        _error: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls
                .lock()
                .await
                .push(format!("rollback:{}", request.agent_id));
            Ok(())
        })
    }

    fn validate_close<'a>(
        &'a self,
        _request: &'a AgentCloseLifecycleRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

fn agent_record(id: &str, status: AgentStatus) -> AgentRecord {
    AgentRecord {
        id: id.to_string(),
        path: format!("/root/{id}"),
        parent_path: Some(AgentPath::ROOT.to_string()),
        role: "executor".to_string(),
        task: format!("inspect {id}"),
        status,
        summary: None,
        error: None,
        reason: None,
        budget_limit_kind: None,
        budget_usage: None,
        depth: 1,
        updated_at: 1,
    }
}

async fn insert_agent(supervisor: &AgentSupervisor, record: AgentRecord) {
    let mut state = supervisor.state.lock().await;
    state
        .path_to_id
        .insert(record.path.clone(), record.id.clone());
    state
        .agents
        .insert(record.id.clone(), AgentEntry::new(record));
}

fn test_run_spec(message: &str) -> AgentRunSpec {
    let mut provider_info = pl_model::ProviderInfo::openai(Some("http://example.invalid".into()));
    provider_info.default_model = "test-model".to_string();
    AgentRunSpec {
        provider: pl_model::create_provider(provider_info).unwrap(),
        reasoning_effort: None,
        config: None,
        mcp_runtime: None,
        lsp_runtime: None,
        workspace_instructions: None,
        instruction_snapshot: None,
        tool_registrar: None,
        workspace_root: PathBuf::from("."),
        options: TurnOptions::default(),
        event_tx: tokio::sync::broadcast::channel(8).0,
        call_id: "call-1".to_string(),
        message: message.to_string(),
        mode: CompileMode::Simple,
        budget: TurnBudget::default(),
        initial_session: crate::CoreSession::new(),
    }
}

#[tokio::test]
async fn agent_ids_are_process_global_across_supervisors() {
    let first = AgentSupervisor::default();
    let second = AgentSupervisor::default();
    let input = AgentSpawnInput {
        task_name: "worker".to_string(),
        message: "inspect".to_string(),
        role: "explorer".to_string(),
        parent_path: Some(AgentPath::ROOT.to_string()),
        session_id: "session".to_string(),
        owned_paths: Vec::new(),
    };

    let first_handle = first
        .spawn_agent(input.clone(), test_run_spec("inspect"))
        .await
        .unwrap();
    let second_handle = second
        .spawn_agent(input, test_run_spec("inspect"))
        .await
        .unwrap();

    assert!(first_handle.id.starts_with("agent-"));
    assert!(second_handle.id.starts_with("agent-"));
    assert_ne!(first_handle.id, second_handle.id);
}

fn normalize_supervisor_source(source: &str) -> String {
    source.replace("\r\n", "\n")
}

fn supervisor_source() -> String {
    normalize_supervisor_source(include_str!("mod.rs"))
}

#[test]
fn agent_input_turn_mode_maps_codex_flags_to_single_policy() {
    assert_eq!(
        AgentInputTurnMode::from_codex_flags(false, false),
        AgentInputTurnMode::QueueOnly
    );
    assert_eq!(
        AgentInputTurnMode::from_codex_flags(true, false),
        AgentInputTurnMode::TriggerTurn
    );
    assert_eq!(
        AgentInputTurnMode::from_codex_flags(false, true),
        AgentInputTurnMode::Interrupt
    );
    assert_eq!(
        AgentInputTurnMode::from_codex_flags(true, true),
        AgentInputTurnMode::Interrupt
    );
}

#[test]
fn agent_input_turn_mode_exposes_queue_and_busy_semantics() {
    assert!(AgentInputTurnMode::QueueOnly.queues_without_start());
    assert!(!AgentInputTurnMode::QueueOnly.interrupts());
    assert!(!AgentInputTurnMode::QueueOnly.queues_when_busy());

    assert!(!AgentInputTurnMode::TriggerTurn.queues_without_start());
    assert!(!AgentInputTurnMode::TriggerTurn.interrupts());
    assert!(AgentInputTurnMode::TriggerTurn.queues_when_busy());

    assert!(!AgentInputTurnMode::Interrupt.queues_without_start());
    assert!(AgentInputTurnMode::Interrupt.interrupts());
    assert!(!AgentInputTurnMode::Interrupt.queues_when_busy());
}

#[test]
fn agent_input_submission_builds_send_input_output() {
    let queued = AgentInputSubmission::queued().into_send_input_output(
        "/root/worker".to_string(),
        AgentStatus::Waiting,
        false,
    );
    let started = AgentInputSubmission::started("turn-1").into_send_input_output(
        "/root/worker".to_string(),
        AgentStatus::Running,
        true,
    );

    assert_eq!(
        queued,
        crate::tool::AgentControlSendInputOutput {
            target: "/root/worker".to_string(),
            status: AgentStatus::Waiting,
            interrupt: false,
            queued: true,
            turn_id: None,
        }
    );
    assert_eq!(
        started,
        crate::tool::AgentControlSendInputOutput {
            target: "/root/worker".to_string(),
            status: AgentStatus::Running,
            interrupt: true,
            queued: false,
            turn_id: Some("turn-1".to_string()),
        }
    );
}

#[test]
fn agent_wait_outcome_builds_wait_agent_output() {
    let output = AgentWaitOutcome::new(true)
        .into_wait_agent_output("{\"pending\":[\"worker\"]}".to_string());

    assert_eq!(
        output,
        crate::tool::AgentControlWaitOutput {
            message: "{\"pending\":[\"worker\"]}".to_string(),
            timed_out: true,
        }
    );
}

#[test]
fn agent_wait_outcome_hides_shared_fields_behind_constructor() {
    let source = supervisor_source();
    let outcome_fields = source
        .split("pub struct AgentWaitOutcome")
        .nth(1)
        .and_then(|text| text.split("impl AgentWaitOutcome").next())
        .expect("AgentWaitOutcome definition");

    assert!(
        source.contains("impl AgentWaitOutcome {\n    /// 使用 timeout 标记创建 wait 输出结果。\n    pub fn new("),
        "AgentWaitOutcome 应通过构造器承载共享输出字段形状"
    );
    assert!(
        !outcome_fields.contains("pub timed_out:"),
        "AgentWaitOutcome 字段不应公开给宿主 adapter 手写"
    );
}

#[test]
fn supervisor_source_normalizes_windows_line_endings_for_shape_checks() {
    let source = normalize_supervisor_source(
        "impl AgentWaitOutcome {\r\n    /// 使用 timeout 标记创建 wait 输出结果。\r\n    pub fn new(",
    );

    assert!(source.contains(
        "impl AgentWaitOutcome {\n    /// 使用 timeout 标记创建 wait 输出结果。\n    pub fn new("
    ));
}

#[test]
fn agent_wait_outcome_builds_group_wait_agent_output() {
    let output = AgentWaitOutcome::new(true).into_group_wait_agent_output(
        vec![json!({ "agentId": "done" })],
        vec![json!({ "agentId": "pending" })],
    );
    let message: serde_json::Value =
        serde_json::from_str(&output.message).expect("wait message json");

    assert!(output.timed_out);
    assert_eq!(
        message,
        json!({
            "completed": [{ "agentId": "done" }],
            "pending": [{ "agentId": "pending" }],
            "timedOut": true,
        })
    );
    assert!(message.get("timed_out").is_none());
}

#[test]
fn agent_input_turn_mode_exposes_dispatch_actions() {
    assert_eq!(
        AgentInputTurnMode::QueueOnly.initial_action(),
        AgentInputInitialAction::Queue
    );
    assert_eq!(
        AgentInputTurnMode::TriggerTurn.initial_action(),
        AgentInputInitialAction::StartTurn
    );
    assert_eq!(
        AgentInputTurnMode::Interrupt.initial_action(),
        AgentInputInitialAction::InterruptThenStart
    );

    assert_eq!(
        AgentInputTurnMode::QueueOnly.busy_action(),
        AgentInputBusyAction::ReturnBusy
    );
    assert_eq!(
        AgentInputTurnMode::TriggerTurn.busy_action(),
        AgentInputBusyAction::Queue
    );
    assert_eq!(
        AgentInputTurnMode::Interrupt.busy_action(),
        AgentInputBusyAction::ReturnBusy
    );
}

#[test]
fn agent_input_queue_preserves_fifo_and_restore_front_order() {
    let mut queue = AgentInputQueue::default();

    assert!(queue.is_empty());
    queue.push("first");
    queue.push("second");

    assert_eq!(queue.len(), 2);
    assert_eq!(queue.pop(), Some("first"));

    queue.restore_front("retry");

    assert_eq!(queue.pop(), Some("retry"));
    assert_eq!(queue.pop(), Some("second"));
    assert_eq!(queue.pop(), None);
}

#[test]
fn agent_input_queue_start_attempt_restores_busy_input_to_front() {
    let mut queue = AgentInputQueue::default();
    queue.push("first");
    queue.push("second");

    let attempt = queue
        .take_start_attempt()
        .expect("start attempt should contain first input");
    assert_eq!(attempt.input(), &"first");
    assert_eq!(queue.len(), 1);

    queue.restore_start_attempt(attempt);

    assert_eq!(queue.pop(), Some("first"));
    assert_eq!(queue.pop(), Some("second"));
}

#[test]
fn agent_wait_completion_completes_without_active_turn() {
    let snapshot = AgentWaitSnapshot::new(
        AgentTurnPresence::NoActiveTurn,
        AgentLifecycleStatusKind::Active,
    );

    assert_eq!(snapshot.completion(), AgentWaitCompletion::Complete);
}

#[test]
fn agent_wait_snapshot_hides_shared_fields_behind_constructor() {
    let source = supervisor_source();
    let wait_snapshot_fields = source
        .split("pub struct AgentWaitSnapshot")
        .nth(1)
        .and_then(|text| text.split("impl AgentWaitSnapshot").next())
        .expect("AgentWaitSnapshot definition");

    assert!(
        source.contains("pub fn new("),
        "AgentWaitSnapshot 应通过构造器承载共享字段形状"
    );
    assert!(
        !wait_snapshot_fields.contains("pub turn_presence:")
            && !wait_snapshot_fields.contains("pub status:"),
        "AgentWaitSnapshot 字段不应公开给宿主 adapter 手写"
    );
}

#[test]
fn agent_wait_completion_completes_for_terminal_or_idle_status() {
    for status in [
        AgentLifecycleStatusKind::Idle,
        AgentLifecycleStatusKind::Completed,
        AgentLifecycleStatusKind::Failed,
        AgentLifecycleStatusKind::Cancelled,
        AgentLifecycleStatusKind::Deleted,
    ] {
        let snapshot = AgentWaitSnapshot {
            turn_presence: AgentTurnPresence::ActiveTurn,
            status,
        };

        assert_eq!(snapshot.completion(), AgentWaitCompletion::Complete);
    }
}

#[test]
fn agent_wait_completion_keeps_active_turn_pending() {
    let snapshot = AgentWaitSnapshot {
        turn_presence: AgentTurnPresence::ActiveTurn,
        status: AgentLifecycleStatusKind::Active,
    };

    assert_eq!(snapshot.completion(), AgentWaitCompletion::Pending);
}

#[test]
fn agent_wait_snapshot_projects_group_progress() {
    assert_eq!(
        AgentWaitSnapshot::from_group_counts(0, 2),
        AgentWaitSnapshot {
            turn_presence: AgentTurnPresence::ActiveTurn,
            status: AgentLifecycleStatusKind::Active,
        }
    );
    assert_eq!(
        AgentWaitSnapshot::from_group_counts(1, 1),
        AgentWaitSnapshot {
            turn_presence: AgentTurnPresence::NoActiveTurn,
            status: AgentLifecycleStatusKind::Completed,
        }
    );
    assert_eq!(
        AgentWaitSnapshot::from_group_counts(0, 0),
        AgentWaitSnapshot {
            turn_presence: AgentTurnPresence::NoActiveTurn,
            status: AgentLifecycleStatusKind::Completed,
        }
    );
}

#[tokio::test]
async fn agent_wait_loop_returns_first_complete_snapshot() {
    let snapshots = Arc::new(Mutex::new(VecDeque::from([
        (
            AgentWaitSnapshot {
                turn_presence: AgentTurnPresence::ActiveTurn,
                status: AgentLifecycleStatusKind::Active,
            },
            "pending".to_string(),
        ),
        (
            AgentWaitSnapshot {
                turn_presence: AgentTurnPresence::NoActiveTurn,
                status: AgentLifecycleStatusKind::Completed,
            },
            "done".to_string(),
        ),
    ])));

    let result = wait_for_agent_completion(
        {
            let snapshots = Arc::clone(&snapshots);
            move || {
                let snapshots = Arc::clone(&snapshots);
                async move { Ok::<_, ()>(snapshots.lock().await.pop_front().expect("snapshot")) }
            }
        },
        AgentWaitLoopOptions::new(Duration::from_secs(1))
            .with_poll_interval(Duration::from_millis(1)),
        &CancellationToken::new(),
    )
    .await
    .expect("wait result");

    assert_eq!(
        result,
        AgentWaitLoopResult {
            value: "done".to_string(),
            timed_out: false,
        }
    );
    assert!(snapshots.lock().await.is_empty());
}

#[tokio::test]
async fn agent_wait_loop_returns_last_snapshot_on_timeout() {
    let result = wait_for_agent_completion(
        || async {
            Ok::<_, ()>((
                AgentWaitSnapshot {
                    turn_presence: AgentTurnPresence::ActiveTurn,
                    status: AgentLifecycleStatusKind::Active,
                },
                "pending".to_string(),
            ))
        },
        AgentWaitLoopOptions::new(Duration::ZERO),
        &CancellationToken::new(),
    )
    .await
    .expect("wait result");

    assert_eq!(
        result,
        AgentWaitLoopResult {
            value: "pending".to_string(),
            timed_out: true,
        }
    );
}

#[tokio::test]
async fn agent_wait_loop_can_wait_without_timeout() {
    let snapshots = Arc::new(Mutex::new(VecDeque::from([
        (
            AgentWaitSnapshot {
                turn_presence: AgentTurnPresence::ActiveTurn,
                status: AgentLifecycleStatusKind::Active,
            },
            "pending".to_string(),
        ),
        (
            AgentWaitSnapshot {
                turn_presence: AgentTurnPresence::NoActiveTurn,
                status: AgentLifecycleStatusKind::Completed,
            },
            "done".to_string(),
        ),
    ])));

    let result = wait_for_agent_completion(
        {
            let snapshots = Arc::clone(&snapshots);
            move || {
                let snapshots = Arc::clone(&snapshots);
                async move { Ok::<_, ()>(snapshots.lock().await.pop_front().expect("snapshot")) }
            }
        },
        AgentWaitLoopOptions::until_complete().with_poll_interval(Duration::from_millis(1)),
        &CancellationToken::new(),
    )
    .await
    .expect("wait result");

    assert_eq!(
        result,
        AgentWaitLoopResult {
            value: "done".to_string(),
            timed_out: false,
        }
    );
    assert!(snapshots.lock().await.is_empty());
}

#[tokio::test]
async fn agent_wait_loop_reports_cancelled() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let err = wait_for_agent_completion(
        || async {
            Ok::<_, ()>((
                AgentWaitSnapshot {
                    turn_presence: AgentTurnPresence::ActiveTurn,
                    status: AgentLifecycleStatusKind::Active,
                },
                "pending".to_string(),
            ))
        },
        AgentWaitLoopOptions::new(Duration::from_secs(1)),
        &cancellation,
    )
    .await
    .expect_err("cancelled");

    assert_eq!(err, AgentWaitLoopError::Cancelled);
}

#[test]
fn agent_turn_start_readiness_allows_idle_and_restartable_statuses() {
    for status in [
        AgentLifecycleStatusKind::Idle,
        AgentLifecycleStatusKind::Completed,
        AgentLifecycleStatusKind::Failed,
        AgentLifecycleStatusKind::Cancelled,
    ] {
        let snapshot = AgentTurnStartSnapshot::new(status);

        assert_eq!(snapshot.readiness(), AgentTurnStartReadiness::Ready);
    }
}

#[test]
fn agent_turn_start_snapshot_hides_shared_fields_behind_constructor() {
    let source = supervisor_source();
    let turn_start_fields = source
        .split("pub struct AgentTurnStartSnapshot")
        .nth(1)
        .and_then(|text| text.split("impl AgentTurnStartSnapshot").next())
        .expect("AgentTurnStartSnapshot definition");

    assert!(
        source.contains("impl AgentTurnStartSnapshot {\n    /// 使用生命周期状态创建 turn start 快照。\n    pub fn new("),
        "AgentTurnStartSnapshot 应通过构造器承载共享字段形状"
    );
    assert!(
        !turn_start_fields.contains("pub status:"),
        "AgentTurnStartSnapshot 字段不应公开给宿主 adapter 手写"
    );
}

#[test]
fn agent_turn_start_readiness_rejects_active_and_deleted_statuses() {
    for status in [
        AgentLifecycleStatusKind::Active,
        AgentLifecycleStatusKind::Deleted,
    ] {
        let snapshot = AgentTurnStartSnapshot::new(status);

        assert_eq!(snapshot.readiness(), AgentTurnStartReadiness::Busy);
    }
}

#[tokio::test]
async fn followup_capacity_failure_does_not_mutate_agent_mailbox_or_status() {
    let supervisor = AgentSupervisor::default();
    supervisor.configure_limits(1, 3).await;
    insert_agent(&supervisor, agent_record("worker", AgentStatus::Waiting)).await;
    let _active_guard = supervisor.reserve_agent_execution().unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let result = supervisor
        .send_message(AgentMessageRequest {
            current_path: AgentPath::ROOT,
            target: "/root/worker",
            message: "continue".to_string(),
            mode: AgentMessageMode::TriggerTurn,
            run_spec: Some(test_run_spec("continue")),
            event_tx: &event_tx,
            call_id: "call-1".to_string(),
        })
        .await;

    assert!(matches!(result, Err(PureError::AgentLimitReached { .. })));
    let state = supervisor.state.lock().await;
    let entry = state.agents.get("worker").unwrap();
    assert_eq!(entry.record.status, AgentStatus::Waiting);
    assert!(entry.mailbox.is_empty());
    assert!(entry.task.is_none());
}

#[tokio::test]
async fn spawn_start_failure_rolls_back_lifecycle_and_supervisor_entry() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let supervisor = AgentSupervisor::default();
    supervisor.set_lifecycle_hook(Arc::new(RecordingSpawnLifecycleHook {
        calls: Arc::clone(&calls),
    }));
    supervisor.configure_limits(1, 1).await;
    let _active_guard = supervisor.reserve_agent_execution().unwrap();

    let result = supervisor
        .spawn_agent(
            AgentSpawnInput {
                task_name: "implement".to_string(),
                message: "implement".to_string(),
                role: "executor".to_string(),
                parent_path: Some(AgentPath::ROOT.to_string()),
                session_id: "session-1".to_string(),
                owned_paths: vec!["code/pl-core/**".to_string()],
            },
            test_run_spec("implement"),
        )
        .await;

    assert!(matches!(result, Err(PureError::AgentLimitReached { .. })));
    assert!(supervisor.list_agents(None).await.unwrap().is_empty());
    let calls = calls.lock().await.clone();
    let agent_id = calls[0].strip_prefix("prepare:").unwrap().to_string();
    assert!(agent_id.starts_with("agent-"));
    assert_eq!(
        calls,
        vec![
            format!("prepare:{agent_id}"),
            format!("activate:{agent_id}"),
            format!("rollback:{agent_id}"),
        ]
    );
}

#[tokio::test]
async fn spawn_start_failure_reports_every_rollback_failure() {
    let repo_root = std::env::temp_dir().join("pure-supervisor-spawn-rollback");
    let worktree = WorktreeCreateSpec {
        repo_root: repo_root.clone(),
        path: repo_root.join(".pure/worktrees/agent-1"),
        branch: "pure-agent-agent-1".to_string(),
        base_commit: "HEAD".to_string(),
    };
    let backend_calls = Arc::new(Mutex::new(Vec::new()));
    let hook_calls = Arc::new(Mutex::new(Vec::new()));
    let supervisor = AgentSupervisor {
        worktree: WorktreeManager::with_backend(
            repo_root.clone(),
            Arc::new(FailingSupervisorWorktreeBackend {
                calls: Arc::clone(&backend_calls),
            }),
        ),
        ..AgentSupervisor::default()
    };
    supervisor.set_lifecycle_hook(Arc::new(FailingRollbackLifecycleHook {
        calls: Arc::clone(&hook_calls),
        worktree,
    }));
    supervisor.configure_limits(1, 1).await;
    let _active_guard = supervisor.reserve_agent_execution().unwrap();

    let error = supervisor
        .spawn_agent(
            AgentSpawnInput {
                task_name: "implement".to_string(),
                message: "implement".to_string(),
                role: "executor".to_string(),
                parent_path: Some(AgentPath::ROOT.to_string()),
                session_id: "session-1".to_string(),
                owned_paths: vec!["code/pl-core/**".to_string()],
            },
            test_run_spec("implement"),
        )
        .await
        .expect_err("spawn must surface rollback failures");

    assert!(supervisor.list_agents(None).await.unwrap().is_empty());
    assert_eq!(
        backend_calls.lock().await.as_slice(),
        ["create", "remove:true", "delete_branch"]
    );
    let hook_calls = hook_calls.lock().await.clone();
    let agent_id = hook_calls[0].strip_prefix("prepare:").unwrap().to_string();
    assert!(agent_id.starts_with("agent-"));
    assert_eq!(
        hook_calls,
        [
            format!("prepare:{agent_id}"),
            format!("activate:{agent_id}"),
            format!("hook_rollback:{agent_id}"),
        ]
    );
    let error = error.to_string();
    for expected in [
        PureError::AgentLimitReached { max_agents: 1 }.to_string(),
        "remove cleanup failed".to_string(),
        "branch cleanup failed".to_string(),
        "hook rollback failed".to_string(),
    ] {
        assert!(
            error.contains(&expected),
            "missing `{expected}` in `{error}`"
        );
    }
    std::fs::remove_dir_all(repo_root).ok();
}

#[tokio::test]
async fn close_agent_waits_for_cancelled_task_to_finish() {
    let supervisor = AgentSupervisor::default();
    let token = CancellationToken::new();
    let task_token = token.clone();
    let (done_tx, done_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        task_token.cancelled().await;
        tokio::time::sleep(Duration::from_millis(25)).await;
        let _ = done_tx.send(());
    });
    let record = agent_record("worker", AgentStatus::Running);
    {
        let mut state = supervisor.state.lock().await;
        state
            .path_to_id
            .insert(record.path.clone(), record.id.clone());
        let mut entry = AgentEntry::new(record.clone());
        entry.cancellation_token = Some(token);
        entry.task = Some(handle);
        state.agents.insert(record.id.clone(), entry);
    }
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    supervisor
        .close_agent(
            AgentPath::ROOT,
            "/root/worker",
            "test close",
            &event_tx,
            "call-1".to_string(),
            crate::agent::worktree::CloseDisposition::Discard,
        )
        .await
        .unwrap();

    assert!(timeout(Duration::from_millis(10), done_rx).await.is_ok());
    let state = supervisor.state.lock().await;
    let entry = state.agents.get("worker").unwrap();
    assert_eq!(entry.record.status, AgentStatus::Shutdown);
    assert!(entry.task.is_none());
}

#[tokio::test]
async fn resume_agent_reactivates_shutdown_agent() {
    let supervisor = AgentSupervisor::default();
    let token = CancellationToken::new();
    let handle = tokio::spawn(async {});
    let mut record = agent_record("worker", AgentStatus::Shutdown);
    record.error = Some("provider returned status 429".to_string());
    record.reason = Some("closed for retry".to_string());
    {
        let mut state = supervisor.state.lock().await;
        state
            .path_to_id
            .insert(record.path.clone(), record.id.clone());
        let mut entry = AgentEntry::new(record.clone());
        entry.cancellation_token = Some(token);
        entry.task = Some(handle);
        state.agents.insert(record.id.clone(), entry);
    }
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);

    let resumed = supervisor
        .resume_agent(AgentPath::ROOT, "/root/worker", &event_tx)
        .await
        .unwrap();

    assert_eq!(
        resumed,
        AgentRecord {
            status: AgentStatus::Waiting,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            updated_at: resumed.updated_at,
            ..record
        }
    );
    let state = supervisor.state.lock().await;
    let entry = state.agents.get("worker").unwrap();
    assert_eq!(entry.record, resumed);
    assert!(entry.cancellation_token.is_none());
    assert!(entry.task.is_none());
    drop(state);

    let event = event_rx.recv().await.unwrap();
    match event {
        pl_trace::AgentEvent::AgentStateChanged {
            id,
            path,
            parent_path,
            role,
            task,
            status,
            summary,
            depth,
            error,
            reason,
            budget_limit_kind,
            budget_usage,
            updated_at,
        } => {
            assert_eq!(
                AgentRecord {
                    id,
                    path,
                    parent_path,
                    role,
                    task,
                    status,
                    summary,
                    error,
                    reason,
                    budget_limit_kind,
                    budget_usage,
                    depth,
                    updated_at,
                },
                resumed
            );
        }
        _ => panic!("expected agent state change event"),
    }
}

#[tokio::test]
async fn resume_agent_rejects_active_agent() {
    let supervisor = AgentSupervisor::default();
    insert_agent(&supervisor, agent_record("worker", AgentStatus::Waiting)).await;
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let result = supervisor
        .resume_agent(AgentPath::ROOT, "/root/worker", &event_tx)
        .await;

    assert!(matches!(
        result,
        Err(PureError::ToolExecutionFailed { tool, error })
            if tool == "resume_agent" && error == "target agent /root/worker is already waiting"
    ));
}

use super::*;
use crate::tool::{ToolBudgetTiming, ToolCachePolicy, ToolRuntimeLockPolicy};
use pretty_assertions::assert_eq;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct ProviderCallIdEchoTool;

impl Tool for ProviderCallIdEchoTool {
    fn name(&self) -> &str {
        "provider_call_id_echo"
    }

    fn description(&self) -> &str {
        "Returns the stable provider call identity from ToolContext"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            Ok(ToolOutput {
                description: context.provider_call_id.unwrap_or_default(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

#[derive(Debug)]
struct BudgetPausedWaitTool;

#[derive(Debug)]
struct FailingApplyPatchTool {
    executions: std::sync::Arc<AtomicUsize>,
}

#[derive(Debug)]
struct CountingSpawnAgentTool {
    executions: std::sync::Arc<AtomicUsize>,
}

#[derive(Debug)]
struct CountingExecTool {
    executions: std::sync::Arc<AtomicUsize>,
}

#[derive(Debug)]
struct CountingCacheableTool {
    executions: std::sync::Arc<AtomicUsize>,
}

#[derive(Debug)]
struct BatchFailingReadTool {
    executions: std::sync::Arc<AtomicUsize>,
    first_started: std::sync::Arc<tokio::sync::Notify>,
    release_first: std::sync::Arc<tokio::sync::Notify>,
}

#[derive(Debug)]
struct BatchEpochProcessTool {
    first_started: std::sync::Arc<tokio::sync::Notify>,
    release_first: std::sync::Arc<tokio::sync::Notify>,
}

impl Tool for FailingApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Test-only failing patch tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" },
                "cwd": { "type": "string" }
            },
            "required": ["input"],
            "additionalProperties": false
        })
    }

    fn effect(&self) -> Option<crate::ToolEffect> {
        None
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        _context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Err(PureError::ToolExecutionFailed {
                tool: "apply_patch".to_string(),
                error: "failed to find expected lines".to_string(),
            })
        })
    }
}

impl Tool for CountingSpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Test-only agent spawn tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" },
                "role": { "type": "string" },
                "forkTurns": { "type": "string" },
                "metadata": { "type": "object" }
            },
            "required": ["message", "role"],
            "additionalProperties": false
        })
    }

    fn effect(&self) -> Option<crate::ToolEffect> {
        Some(crate::ToolEffect::AgentControl)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let execution = self.executions.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(ToolOutput {
                description: serde_json::json!({
                    "agentId": format!("agent-{execution}"),
                    "message": input.arguments["message"],
                })
                .to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

impl Tool for CountingExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Test-only command tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" },
                "cwd": { "type": "string" }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput {
                description: serde_json::json!({
                    "status": "completed",
                    "command": input.arguments["command"],
                })
                .to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

impl Tool for CountingCacheableTool {
    fn name(&self) -> &str {
        "cacheable_read"
    }

    fn description(&self) -> &str {
        "Test-only cacheable read tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn cache_policy(&self, _arguments: &serde_json::Value) -> ToolCachePolicy {
        ToolCachePolicy::UntilWorkspaceMutation
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput {
                description: serde_json::json!({
                    "path": input.arguments["path"],
                    "content": "x".repeat(8_192),
                })
                .to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

impl Tool for BatchFailingReadTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Test-only deterministic read failure"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn cache_policy(&self, _arguments: &serde_json::Value) -> ToolCachePolicy {
        ToolCachePolicy::UntilWorkspaceMutation
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        ToolRuntimeLockPolicy::Exclusive
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.executions.fetch_add(1, Ordering::SeqCst);
            if context.provider_call_id.as_deref() == Some("read-call-1") {
                self.first_started.notify_one();
                self.release_first.notified().await;
            }
            Err(PureError::ToolExecutionFailed {
                tool: "read_file".to_string(),
                error: "startLine exceeds file length".to_string(),
            })
        })
    }
}

impl Tool for BatchEpochProcessTool {
    fn name(&self) -> &str {
        "batch_epoch_process"
    }

    fn description(&self) -> &str {
        "Test-only process effect between duplicate reads"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn effect(&self) -> Option<crate::ToolEffect> {
        Some(crate::ToolEffect::Process)
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        ToolRuntimeLockPolicy::None
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.first_started.notified().await;
            context
                .tool_cache
                .record_effect(Some(crate::ToolEffect::Process), true);
            self.release_first.notify_one();
            Ok(ToolOutput {
                description: "epoch advanced".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

impl Tool for BudgetPausedWaitTool {
    fn name(&self) -> &str {
        "wait_agents"
    }

    fn description(&self) -> &str {
        "Test-only blocking wait"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn budget_timing(&self) -> ToolBudgetTiming {
        ToolBudgetTiming::PauseWhenOnlyScheduledTool
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        _context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            Ok(ToolOutput {
                description: "progress".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

fn read_file_result_text(result: Option<&str>) -> String {
    serde_json::from_str::<serde_json::Value>(result.expect("tool result"))
        .expect("read_file json")
        .get("text")
        .and_then(serde_json::Value::as_str)
        .expect("text")
        .to_string()
}

#[tokio::test]
async fn identical_apply_patch_arguments_with_distinct_call_ids_execute_independently() {
    let executions = std::sync::Arc::new(AtomicUsize::new(0));
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(FailingApplyPatchTool {
        executions: std::sync::Arc::clone(&executions),
    });
    let arguments = serde_json::json!({
        "cwd": ".",
        "input": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch"
    });
    let calls = [
        ToolCall::function(
            "patch-item-1",
            "apply_patch",
            arguments.clone(),
            Some("patch-call-1".to_string()),
        ),
        ToolCall::function(
            "patch-item-2",
            "apply_patch",
            arguments.clone(),
            Some("patch-call-2".to_string()),
        ),
        ToolCall::function(
            "patch-item-3",
            "apply_patch",
            serde_json::json!({
                "cwd": ".",
                "input": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+different\n*** End Patch"
            }),
            Some("patch-call-3".to_string()),
        ),
    ];
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &calls,
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(executions.load(Ordering::SeqCst), 3);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].status, TracePartStatus::Failed);
    assert_eq!(records[1].status, TracePartStatus::Failed);
    assert_eq!(records[2].status, TracePartStatus::Failed);
}

#[tokio::test]
async fn identical_spawn_agent_arguments_with_distinct_call_ids_execute_independently() {
    let executions = std::sync::Arc::new(AtomicUsize::new(0));
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(CountingSpawnAgentTool {
        executions: std::sync::Arc::clone(&executions),
    });
    let assignment = serde_json::json!({
        "message": "Inspect component A without modifying files.",
        "role": "explorer",
        "forkTurns": "none",
        "metadata": { "scope": "src/component-a" }
    });
    let calls = [
        ToolCall::function(
            "spawn-item-1",
            "spawn_agent",
            assignment.clone(),
            Some("spawn-call-1".to_string()),
        ),
        ToolCall::function(
            "spawn-item-2",
            "spawn_agent",
            assignment,
            Some("spawn-call-2".to_string()),
        ),
        ToolCall::function(
            "spawn-item-3",
            "spawn_agent",
            serde_json::json!({
                "message": "Inspect component B without modifying files.",
                "role": "explorer",
                "forkTurns": "none",
                "metadata": { "scope": "src/component-b" }
            }),
            Some("spawn-call-3".to_string()),
        ),
    ];
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &calls,
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(executions.load(Ordering::SeqCst), 3);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].status, TracePartStatus::Completed);
    assert_eq!(records[1].status, TracePartStatus::Completed);
    assert_eq!(records[2].status, TracePartStatus::Completed);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&records[1].result).unwrap()["agentId"],
        "agent-2"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&records[2].result).unwrap()["agentId"],
        "agent-3"
    );
}

#[tokio::test]
async fn identical_exec_arguments_with_distinct_call_ids_execute_independently() {
    let executions = std::sync::Arc::new(AtomicUsize::new(0));
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(CountingExecTool {
        executions: std::sync::Arc::clone(&executions),
    });
    let command = serde_json::json!({
        "command": "verify component-a",
        "cwd": "."
    });
    let calls = [
        ToolCall::function(
            "exec-item-1",
            "exec",
            command.clone(),
            Some("exec-call-1".to_string()),
        ),
        ToolCall::function(
            "exec-item-2",
            "exec",
            command.clone(),
            Some("exec-call-2".to_string()),
        ),
        ToolCall::function(
            "exec-item-3",
            "exec",
            command,
            Some("exec-call-3".to_string()),
        ),
        ToolCall::function(
            "exec-item-4",
            "exec",
            serde_json::json!({
                "command": "verify component-b",
                "cwd": "."
            }),
            Some("exec-call-4".to_string()),
        ),
    ];
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &calls,
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(executions.load(Ordering::SeqCst), 4);
    assert_eq!(records.len(), 4);
    for record in &records {
        assert_eq!(record.status, TracePartStatus::Completed);
    }

    let repeated_response = ToolCall::function(
        "exec-item-5",
        "exec",
        serde_json::json!({
            "command": "verify component-a",
            "cwd": "."
        }),
        Some("exec-call-5".to_string()),
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-2".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));
    let records = execute_tool_calls(
        &[repeated_response],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(executions.load(Ordering::SeqCst), 5);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Completed);
}

#[tokio::test]
async fn identical_cacheable_calls_return_compact_receipts_per_provider_response() {
    let executions = std::sync::Arc::new(AtomicUsize::new(0));
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(CountingCacheableTool {
        executions: std::sync::Arc::clone(&executions),
    });
    let arguments = serde_json::json!({ "path": "src/lib.rs" });
    let calls = [
        ToolCall::function(
            "read-item-1",
            "cacheable_read",
            arguments.clone(),
            Some("read-call-1".to_string()),
        ),
        ToolCall::function(
            "read-item-2",
            "cacheable_read",
            arguments,
            Some("read-call-2".to_string()),
        ),
        ToolCall::function(
            "read-item-3",
            "cacheable_read",
            serde_json::json!({ "path": "src/main.rs" }),
            Some("read-call-3".to_string()),
        ),
    ];
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &calls,
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(executions.load(Ordering::SeqCst), 2);
    assert_eq!(records.len(), 3);
    assert!(records[0].result.len() > 8_000);
    let duplicate = serde_json::from_str::<serde_json::Value>(&records[1].result).unwrap();
    assert_eq!(duplicate["status"], "duplicateSuppressed");
    assert_eq!(duplicate["reusedFromCallId"], "read-call-1");
    assert_eq!(duplicate["scope"], "providerResponse");
    assert!(records[1].result.len() < 256);
    assert!(records[2].result.len() > 8_000);
}

#[tokio::test]
async fn provider_response_uses_one_cache_epoch_across_concurrent_process_effect() {
    let executions = std::sync::Arc::new(AtomicUsize::new(0));
    let first_started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release_first = std::sync::Arc::new(tokio::sync::Notify::new());
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(BatchFailingReadTool {
        executions: std::sync::Arc::clone(&executions),
        first_started: std::sync::Arc::clone(&first_started),
        release_first: std::sync::Arc::clone(&release_first),
    });
    core.register_tool(BatchEpochProcessTool {
        first_started,
        release_first,
    });
    let arguments = serde_json::json!({ "path": "missing.rs" });
    let calls = [
        ToolCall::function(
            "read-item-1",
            "read_file",
            arguments.clone(),
            Some("read-call-1".to_string()),
        ),
        ToolCall::function(
            "process-item",
            "batch_epoch_process",
            serde_json::json!({}),
            Some("process-call".to_string()),
        ),
        ToolCall::function(
            "read-item-2",
            "read_file",
            arguments,
            Some("read-call-2".to_string()),
        ),
    ];
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &calls,
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].status, TracePartStatus::Failed);
    assert_eq!(records[1].status, TracePartStatus::Completed);
    assert_eq!(records[2].status, TracePartStatus::Completed);
    let duplicate = serde_json::from_str::<serde_json::Value>(&records[2].result).unwrap();
    assert_eq!(duplicate["status"], "duplicateSuppressed");
    assert_eq!(duplicate["reusedFromCallId"], "read-call-1");
    assert_eq!(duplicate["scope"], "providerResponse");
}

#[tokio::test]
async fn invalid_function_arguments_are_returned_to_the_model_without_running_the_tool() {
    let core = TurnEngine::default_provider().unwrap();
    let tool_call = ToolCall::invalid_function(
        "call-1",
        "github_api_request",
        "{\"method\":\"POST\"\n\"path\":\"/repos/o/r/pulls/1/reviews\"}",
        "expected `,` or `}` at line 2 column 1",
        None,
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Failed);
    assert!(records[0].result.contains("Invalid JSON arguments"));
    assert!(records[0].result.contains("github_api_request"));
}

#[tokio::test]
async fn single_wait_agents_call_pauses_active_wall_clock_budget() {
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(BudgetPausedWaitTool);
    let tool_call = ToolCall::function("wait-1", "wait_agents", serde_json::json!({}), None);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(5));

    execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert!(budget.check_wall_clock().is_ok());
    assert_eq!(budget.usage().wait_calls, 1);
}

#[tokio::test]
async fn mixed_tool_batch_keeps_wait_agents_time_in_active_budget() {
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(BudgetPausedWaitTool);
    core.register_tool(ProviderCallIdEchoTool);
    let calls = [
        ToolCall::function("wait-1", "wait_agents", serde_json::json!({}), None),
        ToolCall::function(
            "echo-1",
            "provider_call_id_echo",
            serde_json::json!({}),
            None,
        ),
    ];
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(5));

    execute_tool_calls(
        &calls,
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert!(budget.check_wall_clock().is_err());
    assert_eq!(budget.usage().wait_calls, 1);
}

#[tokio::test]
async fn tool_context_uses_item_id_when_provider_call_id_is_missing() {
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(ProviderCallIdEchoTool);
    let tool_call = ToolCall::function(
        "chat-tool-call-1",
        "provider_call_id_echo",
        serde_json::json!({}),
        None,
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].result, "chat-tool-call-1");
    assert_eq!(records[0].call_id, None);
    let terminal_tool = recorder
        .drain()
        .into_iter()
        .find_map(|event| match event.kind {
            TraceEventKind::TracePartCompleted { item } => item.tool,
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("terminal tool trace");
    assert_eq!(terminal_tool.call_id.as_deref(), Some("chat-tool-call-1"));
}

#[tokio::test]
async fn tool_execution_reuses_streamed_trace_part() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace_root = std::env::temp_dir().join(format!("pure-tool-reuse-{unique}"));
    tokio::fs::create_dir_all(&workspace_root).await.unwrap();
    tokio::fs::write(workspace_root.join("note.txt"), "provider item reuse")
        .await
        .unwrap();
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ReadFile));
    let tool_call = ToolCall::function(
        "provider-item-1",
        "read_file",
        serde_json::json!({"path": "note.txt"}),
        Some("call-1".to_string()),
    );
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let streamed_item = recorder.tool_item(
        "turn-1",
        "turn-1-provider-item-1",
        "read_file".to_string(),
        "{\"path\":\"note.txt\"}".to_string(),
        Some("call-1".to_string()),
        Some("provider-item-1".to_string()),
    );
    recorder.start_item(streamed_item);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();
    let terminal_tool = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
                if item.kind == TracePartKind::Tool && item.item_id == "turn-1-provider-item-1" =>
            {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("completed tool item");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Completed);
    assert_eq!(terminal_tool.status, TracePartStatus::Completed);
    let tool = terminal_tool.tool.as_ref().expect("tool trace metadata");
    assert_eq!(tool.call_id.as_deref(), Some("call-1"));
    assert_eq!(tool.provider_item_id.as_deref(), Some("provider-item-1"));
    assert_eq!(tool.arguments, "{\"path\":\"note.txt\"}");
    assert_eq!(
        read_file_result_text(tool.result.as_deref()),
        "provider item reuse"
    );
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Approved,
            TracePartStatus::Running,
            TracePartStatus::Completed,
        ]
    );
    assert!(runtime_progress_texts(&mut event_rx).is_empty());
    let _ = tokio::fs::remove_dir_all(workspace_root).await;
}

#[tokio::test]
async fn tool_execution_reuses_streamed_trace_part_when_provider_id_arrives_late() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace_root = std::env::temp_dir().join(format!("pure-tool-late-provider-{unique}"));
    tokio::fs::create_dir_all(&workspace_root).await.unwrap();
    tokio::fs::write(workspace_root.join("note.txt"), "late provider id")
        .await
        .unwrap();
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ReadFile));
    let tool_call = ToolCall::function(
        "provider-item-1",
        "read_file",
        serde_json::json!({"path": "note.txt"}),
        Some("call-1".to_string()),
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let streamed_item = recorder.tool_item(
        "turn-1",
        "turn-1-call-1",
        "read_file".to_string(),
        "{\"path\":\"note".to_string(),
        Some("call-1".to_string()),
        None,
    );
    recorder.start_item(streamed_item);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();
    let completed_tool_ids = events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Tool => {
                Some(item.item_id.as_str())
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Completed);
    assert_eq!(completed_tool_ids, vec!["turn-1-call-1"]);
    let terminal_tool = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.item_id == "turn-1-call-1" => {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("completed late-provider tool item");
    let tool = terminal_tool.tool.as_ref().expect("tool trace metadata");
    assert_eq!(tool.call_id.as_deref(), Some("call-1"));
    assert_eq!(tool.provider_item_id.as_deref(), Some("provider-item-1"));
    assert_eq!(
        read_file_result_text(tool.result.as_deref()),
        "late provider id"
    );
    assert_eq!(
        tool_statuses(&events, "turn-1-call-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Approved,
            TracePartStatus::Running,
            TracePartStatus::Completed,
        ]
    );
    assert!(tool_statuses(&events, "turn-1-provider-item-1").is_empty());
    let _ = tokio::fs::remove_dir_all(workspace_root).await;
}

#[tokio::test]
async fn tool_runtime_deltas_use_trace_part_id() {
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(DeltaEchoTool);
    let tool_call = ToolCall::function(
        "provider-item-1",
        "delta_echo",
        serde_json::json!({}),
        Some("call-1".to_string()),
    );
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();
    let mut live_events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        live_events.push(event);
    }
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Completed);
    assert_eq!(
        live_tool_result_deltas(&live_events, "turn-1-provider-item-1"),
        vec!["runtime delta".to_string()]
    );
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Approved,
            TracePartStatus::Running,
            TracePartStatus::Completed,
        ]
    );
}

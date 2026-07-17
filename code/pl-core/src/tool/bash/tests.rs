use super::*;
use pretty_assertions::assert_eq;

fn tool_input(command: &str, session_id: &str, tool_id: &str) -> ToolInput {
    ToolInput {
        arguments: serde_json::json!({ "command": command }),
        session_id: session_id.to_string(),
        tool_id: tool_id.to_string(),
        revision_base: 0,
    }
}

fn test_tool() -> BashTool {
    let root = std::env::temp_dir().join(format!(
        "pure-test-tool-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    BashTool::new(root)
}

fn shared_tools() -> (BashTool, WriteStdinTool) {
    let manager = CommandProcessManager::default();
    let bash = test_tool().with_process_manager(manager.clone());
    let stdin = WriteStdinTool::new(manager);
    (bash, stdin)
}

fn command_json(output: &ToolOutput) -> CommandJsonOutput {
    serde_json::from_str(&output.description).unwrap()
}

fn test_context() -> ToolContext {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    test_context_with_sender(event_tx)
}

fn test_context_with_sender(event_tx: pl_trace::AgentEventSender) -> ToolContext {
    ToolContext {
        event_tx,
        options: crate::turn::TurnOptions::default(),
        workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
        workspace_root: std::env::temp_dir(),
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        lsp_runtime: None,
        parent_session: std::sync::Arc::new(crate::AgentSession::new()),
        working_set: crate::TurnWorkingSetHandle::default(),
        tool_cache: crate::TurnToolCacheHandle::default(),
    }
}

fn sleep_then_echo_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "Start-Sleep -Milliseconds 700; Write-Output 'done'"
    } else {
        "sleep 0.7; echo done"
    }
}

fn long_sleep_then_echo_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "Start-Sleep -Seconds 10; Write-Output 'done'"
    } else {
        "sleep 10; echo done"
    }
}

fn stdin_echo_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "$line = [Console]::In.ReadLine(); Write-Output ('got:' + $line)"
    } else {
        "read line; echo got:$line"
    }
}

fn stderr_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "Write-Error 'err'"
    } else {
        "echo err >&2"
    }
}

fn collect_tool_result_stream(
    event_rx: &mut tokio::sync::broadcast::Receiver<pl_trace::AgentEvent>,
) -> (String, u64) {
    let mut streamed = String::new();
    let mut revision = 0;
    while let Ok(event) = event_rx.try_recv() {
        if let pl_trace::AgentEvent::TracePartDelta { event } = event
            && let pl_trace::TraceDelta::ToolResult { delta } = event.delta
        {
            revision = revision.max(event.revision);
            streamed.push_str(&delta);
        }
    }
    (streamed, revision)
}

fn large_output_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "for ($i = 0; $i -lt 250; $i++) { Write-Output 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }"
    } else {
        "printf '%*s' 5000 '' | tr ' ' a"
    }
}

#[tokio::test]
async fn echoes_hello() {
    let tool = test_tool();
    let output = tool
        .execute(tool_input("echo hello", "s1", "t1"), test_context())
        .await
        .unwrap();
    let result = command_json(&output);

    assert_eq!(result.status, "completed");
    assert!(!output.timed_out);
    assert!(!output.truncated.stdout.was_truncated);
    assert!(output.truncated.stdout.content.contains("hello"));
    assert_eq!(output.exit_code, Some(0));
}

#[tokio::test]
async fn streams_tool_result_delta_for_command_output() {
    let tool = test_tool();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut input = tool_input("echo streaming", "stream-session", "stream-tool");
    input.revision_base = 5;
    let output = tool
        .execute(input, test_context_with_sender(event_tx))
        .await
        .unwrap();

    let (streamed, revision) = collect_tool_result_stream(&mut event_rx);

    assert!(streamed.contains("streaming"));
    assert!(revision > 5);
    assert!(output.runtime_events.iter().any(|event| matches!(
        event,
        ToolRuntimeEvent::ToolResultRevision {
            revision: output_revision
        } if *output_revision >= revision
    )));
}

#[tokio::test]
async fn streams_tool_result_delta_for_stderr_output() {
    let tool = test_tool();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let output = tool
        .execute(
            tool_input(
                stderr_command(),
                "stderr-stream-session",
                "stderr-stream-tool",
            ),
            test_context_with_sender(event_tx),
        )
        .await
        .unwrap();

    let (streamed, revision) = collect_tool_result_stream(&mut event_rx);

    assert!(streamed.contains("[stderr]"));
    assert!(streamed.contains("err"));
    assert!(output.runtime_events.iter().any(|event| matches!(
        event,
        ToolRuntimeEvent::ToolResultRevision {
            revision: output_revision
        } if *output_revision >= revision
    )));
}

#[tokio::test]
async fn captures_stderr() {
    let tool = test_tool();
    let output = tool
        .execute(tool_input(stderr_command(), "s2", "t2"), test_context())
        .await
        .unwrap();

    assert!(output.truncated.stderr.content.contains("err"));
}

#[cfg(windows)]
#[tokio::test]
async fn windows_default_shell_executes_powershell_script() {
    let tool = test_tool();
    let output = tool
        .execute(
            tool_input(
                "if ($PSVersionTable.PSVersion.Major -ge 5) { Write-Output 'powershell-ok' }; (Get-Location).Path",
                "ps-session",
                "ps-tool",
            ),
            test_context(),
        )
        .await
        .unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(output.truncated.stdout.content.contains("powershell-ok"));
    assert!(output.truncated.stdout.content.lines().count() >= 2);
}

#[cfg(windows)]
#[tokio::test]
async fn windows_powershell_captures_unicode_stdout() {
    let tool = test_tool();
    let output = tool
        .execute(
            tool_input("Write-Output '中文输出'", "unicode-session", "unicode-tool"),
            test_context(),
        )
        .await
        .unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(output.truncated.stdout.content.contains("中文输出"));
}

#[tokio::test]
async fn exit_code_nonzero() {
    let tool = test_tool();
    let output = tool
        .execute(tool_input("exit 42", "s3", "t3"), test_context())
        .await
        .unwrap();
    let result = command_json(&output);

    assert_eq!(output.exit_code, Some(42));
    assert_eq!(result.status, "failed");
}

#[tokio::test]
async fn invalid_input_returns_error() {
    let tool = test_tool();
    let result = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({}),
                session_id: "s4".to_string(),
                tool_id: "t4".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn defaults_to_workspace_root_as_current_directory() {
    let tool = test_tool();
    let output = tool
        .execute(
            tool_input("echo marker > cwd-check.txt", "cwd-session", "cwd-tool"),
            test_context(),
        )
        .await
        .unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(tool.workspace_root.join("cwd-check.txt").exists());
    let _ = tokio::fs::remove_file(tool.workspace_root.join("cwd-check.txt")).await;
}

#[tokio::test]
async fn rejects_working_directory_outside_workspace() {
    let tool = test_tool();
    let result = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": "echo no",
                    "workingDirectory": ".."
                }),
                session_id: "cwd-session".to_string(),
                tool_id: "escape-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn relative_working_directory_resolves_from_workspace_root() {
    let tool = test_tool();
    tokio::fs::create_dir_all(tool.workspace_root.join("subdir"))
        .await
        .unwrap();
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": "echo marker > cwd-check.txt",
                    "workingDirectory": "subdir",
                }),
                session_id: "cwd-session".to_string(),
                tool_id: "relative-cwd".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await
        .unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(tool.workspace_root.join("subdir/cwd-check.txt").exists());
    let _ = tokio::fs::remove_dir_all(&tool.workspace_root).await;
}

#[tokio::test]
async fn full_access_allows_working_directory_outside_workspace() {
    let tool = test_tool();
    let outside = tool.workspace_root.parent().unwrap().to_path_buf();
    let mut context = test_context();
    context.options = crate::turn::TurnOptions::default()
        .with_permission_mode(crate::turn::PermissionMode::FullAccess);
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": "echo yes",
                    "workingDirectory": outside,
                }),
                session_id: "cwd-session".to_string(),
                tool_id: "full-access-cwd".to_string(),
                revision_base: 0,
            },
            context,
        )
        .await
        .unwrap();

    assert_eq!(output.exit_code, Some(0));
    let _ = tokio::fs::remove_file(&output.output_file).await;
    let _ = tokio::fs::remove_dir_all(output.output_file.parent().unwrap().parent().unwrap()).await;
}

#[tokio::test]
async fn full_output_saved_to_file() {
    let tool = test_tool();
    let output = tool
        .execute(tool_input("echo test", "s5", "t5"), test_context())
        .await
        .unwrap();

    let content = tokio::fs::read_to_string(&output.output_file)
        .await
        .unwrap();
    assert!(content.contains("=== COMMAND ==="));
    assert!(content.contains("test"));

    let _ = tokio::fs::remove_file(&output.output_file).await;
    let _ = tokio::fs::remove_dir(output.output_file.parent().unwrap()).await;
    let _ = tokio::fs::remove_dir(output.output_file.parent().unwrap().parent().unwrap()).await;
}

#[tokio::test]
async fn output_file_path_follows_convention() {
    let tool = test_tool();
    let output = tool
        .execute(
            tool_input("echo ok", "my-session", "my-tool"),
            test_context(),
        )
        .await
        .unwrap();

    let path = output.output_file;
    assert!(path.ends_with("target/pure/my-session/my-tool/output.log"));

    let _ = tokio::fs::remove_file(&path).await;
    let _ = tokio::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).await;
}

#[tokio::test]
async fn long_command_returns_process_id_then_can_be_polled() {
    let (bash, stdin) = shared_tools();
    let running = bash
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": sleep_then_echo_command(),
                    "yieldTimeMs": 250,
                }),
                session_id: "long-session".to_string(),
                tool_id: "long-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await
        .unwrap();
    let running_result = command_json(&running);

    assert_eq!(running_result.status, "running", "{running_result:?}");
    let process_id = running_result.process_id.unwrap();

    let mut result = None;
    for _ in 0..5 {
        let completed = stdin
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "processId": process_id,
                        "chars": "",
                        "yieldTimeMs": 1500,
                    }),
                    session_id: "long-session".to_string(),
                    tool_id: "poll-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();
        result = Some(command_json(&completed));
        if result.as_ref().unwrap().status == "completed" {
            break;
        }
    }
    let result = result.unwrap();

    assert_eq!(result.status, "completed", "{result:?}");
    assert!(result.stdout.contains("done"));
}

#[tokio::test]
async fn write_stdin_sends_input_to_running_process() {
    let (bash, stdin) = shared_tools();
    let running = bash
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": stdin_echo_command(),
                    "yieldTimeMs": 250,
                }),
                session_id: "stdin-session".to_string(),
                tool_id: "stdin-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await
        .unwrap();
    let running_result = command_json(&running);
    assert_eq!(running_result.status, "running", "{running_result:?}");
    let process_id = running_result.process_id.unwrap();

    let mut result = None;
    for attempt in 0..5 {
        let completed = stdin
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "processId": process_id,
                        "chars": if attempt == 0 { "hello\n" } else { "" },
                        "yieldTimeMs": 3000,
                    }),
                    session_id: "stdin-session".to_string(),
                    tool_id: format!("stdin-write-{attempt}"),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();
        result = Some(command_json(&completed));
        if result.as_ref().unwrap().status == "completed" {
            break;
        }
    }
    let result = result.unwrap();

    assert_eq!(result.status, "completed", "{result:?}");
    assert!(result.stdout.contains("got:hello"));
}

#[tokio::test]
async fn timeout_terminates_background_process() {
    let (tool, stdin) = shared_tools();
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": long_sleep_then_echo_command(),
                    "timeoutSeconds": 1,
                    "yieldTimeMs": 3000,
                }),
                session_id: "timeout-session".to_string(),
                tool_id: "timeout-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await
        .unwrap();
    let mut result = command_json(&output);
    let mut timed_out = output.timed_out || result.timed_out;
    for attempt in 0..8 {
        if result.status == "timedOut" {
            break;
        }
        let Some(process_id) = result.process_id.clone() else {
            break;
        };
        let polled = stdin
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "processId": process_id,
                        "yieldTimeMs": 1000,
                    }),
                    session_id: "timeout-session".to_string(),
                    tool_id: format!("timeout-poll-{attempt}"),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();
        timed_out = timed_out || polled.timed_out;
        result = command_json(&polled);
    }

    assert_eq!(result.status, "timedOut", "{result:?}");
    assert!(timed_out);
    assert_eq!(result.process_id, None);
}

#[tokio::test]
async fn process_limit_returns_recoverable_error() {
    let manager = CommandProcessManager::new(1);
    let bash = test_tool().with_process_manager(manager.clone());
    let stdin = WriteStdinTool::new(manager);
    let first = bash
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": sleep_then_echo_command(),
                    "yieldTimeMs": 250,
                }),
                session_id: "limit-session".to_string(),
                tool_id: "first-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await
        .unwrap();
    let process_id = command_json(&first).process_id.unwrap();

    let second = bash
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": sleep_then_echo_command(),
                    "yieldTimeMs": 250,
                }),
                session_id: "limit-session".to_string(),
                tool_id: "second-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await;

    assert!(second.unwrap_err().to_string().contains("process limit"));

    let _ = stdin
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "processId": process_id,
                    "yieldTimeMs": 3000,
                }),
                session_id: "limit-session".to_string(),
                tool_id: "cleanup-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await;
}

#[tokio::test]
async fn write_stdin_unknown_process_is_recoverable_error() {
    let (_bash, stdin) = shared_tools();
    let result = stdin
        .execute(
            ToolInput {
                arguments: serde_json::json!({ "processId": "missing" }),
                session_id: "missing-session".to_string(),
                tool_id: "missing-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await;

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("not a live process")
    );
}

#[tokio::test]
async fn large_output_is_truncated_in_json_and_saved_to_file() {
    let tool = test_tool();
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": large_output_command(),
                    "maxOutputChars": 100,
                }),
                session_id: "large-session".to_string(),
                tool_id: "large-tool".to_string(),
                revision_base: 0,
            },
            test_context(),
        )
        .await
        .unwrap();
    let result = command_json(&output);
    let file_content = tokio::fs::read_to_string(&output.output_file)
        .await
        .unwrap();

    assert!(result.stdout.len() < 5000);
    assert!(result.stdout.contains("omitted"));
    assert!(file_content.len() > result.stdout.len());
}

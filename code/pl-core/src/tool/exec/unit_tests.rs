use super::*;
use crate::{CommandOutputSizes, CommandOutputTarget, CommandSpawnRequest};
use pretty_assertions::assert_eq;
use std::sync::atomic::{AtomicUsize, Ordering};

fn tool_input(command: &str, session_id: &str, tool_id: &str) -> ToolInput {
    ToolInput {
        arguments: serde_json::json!({ "command": command }),
        session_id: session_id.to_string(),
        tool_id: tool_id.to_string(),
        revision_base: 0,
    }
}

type TestExecTool = ExecTool<LocalCommandBackend>;
type TestWriteStdinTool = WriteStdinTool<LocalCommandBackend>;

fn test_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "pure-test-tool-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn test_backend() -> Arc<LocalCommandBackend> {
    let root = test_root();
    std::fs::create_dir_all(&root).unwrap();
    Arc::new(LocalCommandBackend::new(root))
}

fn test_tool() -> TestExecTool {
    ExecTool::new(test_backend())
}

fn test_tool_with_root() -> (TestExecTool, PathBuf) {
    let root = test_root();
    std::fs::create_dir_all(&root).unwrap();
    let tool = ExecTool::new(Arc::new(LocalCommandBackend::new(root.clone())));
    (tool, root)
}

fn shared_tools() -> (TestExecTool, TestWriteStdinTool) {
    command_tool_pair(test_backend())
}

#[test]
fn empty_poll_uses_runtime_backoff_without_delaying_immediate_snapshots() {
    assert_eq!(poll_yield_duration(None), Duration::from_secs(10));
    assert_eq!(poll_yield_duration(Some(1_000)), Duration::from_secs(10));
    assert_eq!(poll_yield_duration(Some(30_000)), Duration::from_secs(30));
    assert_eq!(poll_yield_duration(Some(0)), Duration::ZERO);
    assert_eq!(yield_duration(Some(1_000)), Duration::from_secs(1));
}

#[cfg(unix)]
fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

#[cfg(unix)]
fn remove_directory_link(link: &std::path::Path) {
    std::fs::remove_file(link).unwrap();
}

#[cfg(windows)]
fn remove_directory_link(link: &std::path::Path) {
    std::fs::remove_dir(link).unwrap();
}

#[derive(Debug)]
struct HostedContractBackend {
    local: LocalCommandBackend,
    publish_count: AtomicUsize,
}

#[tokio::test]
async fn local_backend_rejects_linked_working_directory() {
    let root = test_root();
    let outside = test_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    create_directory_link(&outside, &root.join("linked"));
    let backend = LocalCommandBackend::new(root.clone());

    let error = backend
        .resolve_cwd(Some(std::path::Path::new("linked")), false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("reparse point"), "{error}");
    remove_directory_link(&root.join("linked"));
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn local_backend_resolves_native_non_verbatim_working_directory() {
    let root = test_root();
    std::fs::create_dir_all(&root).unwrap();
    let backend = LocalCommandBackend::new(root.clone());

    let resolved = backend.resolve_cwd(None, false).await.unwrap();

    assert!(!resolved.to_string_lossy().starts_with(r"\\?\"));
    std::fs::remove_dir_all(root).unwrap();
}

impl CommandBackend for HostedContractBackend {
    type Error = PureError;

    async fn resolve_cwd(
        &self,
        cwd: Option<&std::path::Path>,
        allow_workspace_escape: bool,
    ) -> Result<PathBuf, Self::Error> {
        self.local.resolve_cwd(cwd, allow_workspace_escape).await
    }

    async fn output_target(
        &self,
        session_id: &str,
        tool_id: &str,
        call_id: &str,
        command: &str,
    ) -> Result<CommandOutputTarget, Self::Error> {
        let local = self
            .local
            .output_target(session_id, tool_id, call_id, command)
            .await?;
        Ok(CommandOutputTarget::new(
            local.capture_file(),
            PathBuf::from("hosted/output.log"),
        ))
    }

    async fn spawn(
        &self,
        request: CommandSpawnRequest,
    ) -> Result<tokio::process::Child, Self::Error> {
        self.local.spawn(request).await
    }

    async fn publish_output(&self, _target: &CommandOutputTarget) -> Result<(), Self::Error> {
        self.publish_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn collect_output_artifacts(
        &self,
        _target: &CommandOutputTarget,
        _sizes: CommandOutputSizes,
    ) -> Result<Vec<serde_json::Value>, Self::Error> {
        Ok(vec![serde_json::json!({
            "id": "artifact-1",
            "call_id": "call-1",
            "name": "stdout.txt",
            "stream": "stdout",
        })])
    }

    async fn terminate(&self, process_id: &str, host_pid: Option<u32>) {
        self.local.terminate(process_id, host_pid).await;
    }

    fn terminate_sync(&self, process_id: &str, host_pid: Option<u32>) {
        self.local.terminate_sync(process_id, host_pid);
    }
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
        workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        lsp_runtime: None,
        parent_session: std::sync::Arc::new(crate::AgentSession::new()),
        working_set: crate::TurnWorkingSetHandle::default(),
        tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
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
async fn background_output_observer_does_not_keep_turn_event_channel_open() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let observer = ToolResultOutputObserver {
        event_tx: event_tx.downgrade(),
        turn_id: "turn-1".to_string(),
        item_id: "tool-1".to_string(),
        revision_base: 0,
    };

    observer.output_chunk(CommandOutputStream::Stdout, b"running", 1);
    assert!(matches!(
        event_rx.recv().await,
        Ok(pl_trace::AgentEvent::TracePartDelta { .. })
    ));

    drop(event_tx);
    assert!(matches!(
        event_rx.recv().await,
        Err(tokio::sync::broadcast::error::RecvError::Closed)
    ));

    observer.output_chunk(CommandOutputStream::Stdout, b"late output", 2);
}

#[tokio::test]
async fn injected_backend_keeps_the_exec_result_contract() {
    let root = test_root();
    std::fs::create_dir_all(&root).unwrap();
    let backend = Arc::new(HostedContractBackend {
        local: LocalCommandBackend::new(root.clone()),
        publish_count: AtomicUsize::new(0),
    });
    let tool = ExecTool::new(backend.clone());

    let output = tool
        .execute(
            tool_input("echo hosted", "hosted-session", "hosted-tool"),
            test_context(),
        )
        .await
        .unwrap();
    let result = command_json(&output);

    assert!(matches!(
        result.state.final_result(),
        Some(CommandProcessFinalResult::Succeeded { .. })
    ));
    assert_eq!(result.output_file, "hosted/output.log");
    assert_eq!(
        result.output_artifacts,
        vec![serde_json::json!({
            "id": "artifact-1",
            "call_id": "call-1",
            "name": "stdout.txt",
            "stream": "stdout",
        })]
    );
    assert!(output.runtime_events.iter().any(|event| matches!(
        event,
        ToolRuntimeEvent::OutputArtifacts { artifacts }
            if artifacts == &result.output_artifacts
    )));
    assert!(result.stdout.contains("hosted"));
    assert!(backend.publish_count.load(Ordering::Relaxed) >= 1);
    let _ = tokio::fs::remove_dir_all(root).await;
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
async fn defaults_to_workspace_root_as_current_directory() {
    let (tool, root) = test_tool_with_root();
    let output = tool
        .execute(
            tool_input("echo marker > cwd-check.txt", "cwd-session", "cwd-tool"),
            test_context(),
        )
        .await
        .unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(root.join("cwd-check.txt").exists());
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rejects_working_directory_outside_workspace() {
    let tool = test_tool();
    let result = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": "echo no",
                    "cwd": ".."
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
    let (tool, root) = test_tool_with_root();
    tokio::fs::create_dir_all(root.join("subdir"))
        .await
        .unwrap();
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": "echo marker > cwd-check.txt",
                    "cwd": "subdir",
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
    assert!(root.join("subdir/cwd-check.txt").exists());
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn full_access_allows_working_directory_outside_workspace() {
    let (tool, root) = test_tool_with_root();
    let outside = root.parent().unwrap().to_path_buf();
    let mut context = test_context();
    context.options = crate::turn::TurnOptions::default()
        .with_permission_mode(crate::turn::PermissionMode::FullAccess);
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "command": "echo yes",
                    "cwd": outside,
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
#[ignore = "requires ripgrep on the host"]
async fn exec_allows_search_read_and_write_in_read_only_workspace() {
    let (tool, root) = test_tool_with_root();
    tokio::fs::write(root.join("read-only-source.txt"), "read-only fixture\n")
        .await
        .unwrap();
    let mut context = test_context();
    context.workspace = crate::tool::AgentWorkspace::confined(
        root.clone(),
        crate::tool::WorkspaceMutability::ReadOnly,
    );

    let search = tool
        .execute(
            tool_input("rg --version", "read-only", "search"),
            context.clone(),
        )
        .await
        .unwrap();
    assert_eq!(search.exit_code, Some(0));
    assert!(search.truncated.stdout.content.contains("ripgrep"));

    #[cfg(windows)]
    let read_command = "Get-Content -LiteralPath 'read-only-source.txt'";
    #[cfg(not(windows))]
    let read_command = "cat read-only-source.txt";
    let read = tool
        .execute(
            tool_input(read_command, "read-only", "read"),
            context.clone(),
        )
        .await
        .unwrap();
    assert_eq!(read.exit_code, Some(0));
    assert!(read.truncated.stdout.content.contains("read-only fixture"));

    #[cfg(windows)]
    let write_command = "Set-Content -LiteralPath 'shell-write.txt' -Value 'written'";
    #[cfg(not(windows))]
    let write_command = "printf written > shell-write.txt";
    let write = tool
        .execute(tool_input(write_command, "read-only", "write"), context)
        .await
        .unwrap();
    assert_eq!(write.exit_code, Some(0));
    assert_eq!(
        tokio::fs::read_to_string(root.join("shell-write.txt"))
            .await
            .unwrap()
            .trim(),
        "written"
    );

    let _ = tokio::fs::remove_dir_all(root).await;
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
    let (exec, stdin) = shared_tools();
    let running = exec
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

    assert!(
        matches!(&running_result.state, CommandProcessLifecycle::Running(_)),
        "{running_result:?}"
    );
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
        if matches!(
            result.as_ref().unwrap().state.final_result(),
            Some(CommandProcessFinalResult::Succeeded { .. })
        ) {
            break;
        }
    }
    let result = result.unwrap();

    assert!(
        matches!(
            result.state.final_result(),
            Some(CommandProcessFinalResult::Succeeded { .. })
        ),
        "{result:?}"
    );
    assert!(result.stdout.contains("done"));
}

#[tokio::test]
async fn write_stdin_sends_input_to_running_process() {
    let (exec, stdin) = shared_tools();
    let running = exec
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
    assert!(
        matches!(&running_result.state, CommandProcessLifecycle::Running(_)),
        "{running_result:?}"
    );
    let process_id = running_result.process_id.unwrap();

    let mut result = None;
    let mut stdout = String::new();
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
        stdout.push_str(&result.as_ref().unwrap().stdout);
        if matches!(
            result.as_ref().unwrap().state.final_result(),
            Some(CommandProcessFinalResult::Succeeded { .. })
        ) {
            break;
        }
    }
    let result = result.unwrap();

    assert!(
        matches!(
            result.state.final_result(),
            Some(CommandProcessFinalResult::Succeeded { .. })
        ),
        "{result:?}"
    );
    assert!(stdout.contains("got:hello"), "{stdout}");
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
    let mut timed_out = output.timed_out || result.state.is_timed_out();
    for attempt in 0..8 {
        if matches!(
            result.state.final_result(),
            Some(CommandProcessFinalResult::TimedOut)
        ) {
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

    assert!(
        matches!(
            result.state.final_result(),
            Some(CommandProcessFinalResult::TimedOut)
        ),
        "{result:?}"
    );
    assert!(timed_out);
    assert_eq!(result.process_id, None);
}

#[tokio::test]
async fn process_limit_returns_recoverable_error() {
    let manager = CommandProcessManager::with_max_processes(test_backend(), 1);
    let exec = test_tool().with_process_manager(manager.clone());
    let stdin = WriteStdinTool::new(manager);
    let first = exec
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

    let second = exec
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

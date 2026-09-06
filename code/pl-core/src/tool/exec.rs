use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::{OutputStream, PureError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::command::process_manager::*;
use super::command::{CommandBackend, LocalCommandBackend};
use super::truncation::{OutputTruncation, TruncationStrategy};
use super::{StaticTool, ToolCallContext, ToolDirective, ToolPolicy, ToolResult, ToolWorkspace};
use crate::execution_environment::ExecutionEnvironment;
use crate::turn::ToolEffect;

pub const TOOL_EXEC: &str = "exec";
pub const TOOL_WRITE_STDIN: &str = "write_stdin";

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_YIELD_TIME_MS: u64 = 10_000;
const MIN_YIELD_TIME_MS: u64 = 250;
const MAX_YIELD_TIME_MS: u64 = 30_000;
const MAX_MODEL_OUTPUT_CHARS: usize = 64 * 1024;

/// 启动命令并通过统一 workspace backend 执行的工具。
#[derive(Debug, Clone)]
pub struct ExecTool<B>
where
    B: CommandBackend,
{
    truncation: TruncationStrategy,
    default_timeout: Duration,
    process_manager: CommandProcessManager<B>,
    workspace: ToolWorkspace,
}

/// 向 `exec` 启动的后台命令写入 stdin 或轮询输出。
#[derive(Debug, Clone)]
pub struct WriteStdinTool<B>
where
    B: CommandBackend,
{
    process_manager: CommandProcessManager<B>,
}

/// `exec` 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecInput {
    /// The shell command to execute.
    pub command: String,
    /// Optional working directory. Use `.` for the workspace root or a workspace-relative path
    /// such as `src`. SSH execution rejects absolute paths; local absolute paths remain subject to
    /// the active permission and workspace policy.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Optional total timeout in seconds (default: 60).
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub timeout_seconds: Option<u64>,
    /// How long to wait before returning a running process id.
    #[serde(default)]
    pub yield_time_ms: Option<u64>,
    /// Maximum stdout/stderr characters returned to the model.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub max_output_chars: Option<usize>,
}

/// `write_stdin` 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteStdinInput {
    /// Process id returned by a running exec result.
    pub process_id: String,
    /// Text to write; omit or pass an empty string to only wait or poll.
    #[serde(default)]
    pub chars: Option<String>,
    /// How long to wait for process exit; zero returns an immediate snapshot.
    #[serde(default)]
    pub yield_time_ms: Option<u64>,
    /// Maximum stdout/stderr characters returned to the model.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub max_output_chars: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandJsonOutput {
    state: CommandProcessLifecycle,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_id: Option<String>,
    stdout: String,
    stderr: String,
    output_file: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    output_artifacts: Vec<serde_json::Value>,
    message: String,
}

impl<B> ExecTool<B>
where
    B: CommandBackend,
{
    pub fn new(backend: Arc<B>, workspace: ToolWorkspace) -> Self {
        Self {
            truncation: TruncationStrategy::default(),
            default_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            process_manager: CommandProcessManager::new(backend),
            workspace,
        }
    }

    pub fn with_truncation(mut self, strategy: TruncationStrategy) -> Self {
        self.truncation = strategy;
        self
    }

    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_process_manager(mut self, manager: CommandProcessManager<B>) -> Self {
        self.process_manager = manager;
        self
    }

    fn default_max_output_chars(&self) -> usize {
        self.truncation
            .head_limit
            .saturating_add(self.truncation.tail_limit)
    }
}

struct ToolResultOutputObserver {
    emitter: super::ToolOutputDeltaEmitter,
}

impl CommandOutputObserver for ToolResultOutputObserver {
    fn output_chunk(&self, stream: CommandOutputStream, chunk: &[u8], _revision: u64) {
        let stream = match stream {
            CommandOutputStream::Stdout => OutputStream::Stdout,
            CommandOutputStream::Stderr => OutputStream::Stderr,
        };
        let _ = self.emitter.emit(stream, String::from_utf8_lossy(chunk));
    }
}

/// Builds the `exec` and `write_stdin` tools over one shared process manager.
pub fn command_tool_pair<B>(
    backend: Arc<B>,
    workspace: ToolWorkspace,
) -> (ExecTool<B>, WriteStdinTool<B>)
where
    B: CommandBackend,
{
    let manager = CommandProcessManager::new(backend);
    (
        ExecTool {
            truncation: TruncationStrategy::default(),
            default_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            process_manager: manager.clone(),
            workspace,
        },
        WriteStdinTool::new(manager),
    )
}

/// Builds the local command tool pair with an explicit execution environment.
pub fn local_command_tool_pair_with_environment(
    workspace: ToolWorkspace,
    execution_environment: ExecutionEnvironment,
) -> (
    ExecTool<LocalCommandBackend>,
    WriteStdinTool<LocalCommandBackend>,
) {
    let backend = Arc::new(
        LocalCommandBackend::new(workspace.root().to_path_buf())
            .with_execution_environment(execution_environment),
    );
    command_tool_pair(backend, workspace)
}

impl<B> WriteStdinTool<B>
where
    B: CommandBackend,
{
    /// Builds `write_stdin` for an existing shared command process manager.
    pub fn new(process_manager: CommandProcessManager<B>) -> Self {
        Self { process_manager }
    }
}

impl<B> StaticTool for ExecTool<B>
where
    B: CommandBackend,
{
    type Input = ExecInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin(TOOL_EXEC),
            "Start a shell command in the agent workspace and return a compact JSON result. Shell commands are not constrained by a directory Profile's writablePaths; obey the frozen workspace assignment and do not modify project files outside it. If the command is still running after yieldTimeMs, use write_stdin with the returned processId. Full output is saved to a workspace-relative outputFile.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::Process)
    }

    fn execute(
        &self,
        exec_input: ExecInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let timeout = exec_input
                .timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or(self.default_timeout);
            let observer = Arc::new(ToolResultOutputObserver {
                emitter: context.output_delta_emitter(),
            });
            let call_id = context.identity().call_id.clone();
            let snapshot = self
                .process_manager
                .start(CommandStartRequest {
                    command: exec_input.command,
                    cwd: exec_input.cwd,
                    allow_workspace_escape: self.workspace.allows_workspace_escape(&context),
                    timeout,
                    yield_time: yield_duration(exec_input.yield_time_ms),
                    max_output_chars: max_output_chars(
                        exec_input.max_output_chars,
                        self.default_max_output_chars(),
                    ),
                    session_id: context.identity().session_id.clone(),
                    tool_id: context.identity().item_id.clone(),
                    call_id,
                    cancellation_token: context.cancellation_token(),
                    output_observer: Some(observer),
                })
                .await?;

            if let Some(error) = context.take_output_delta_error() {
                return Err(PureError::ToolExecutionFailed {
                    tool: TOOL_EXEC.to_string(),
                    error: error.to_string(),
                });
            }

            tool_output_from_snapshot(snapshot, TOOL_EXEC)
        }
    }
}

impl<B> StaticTool for WriteStdinTool<B>
where
    B: CommandBackend,
{
    type Input = WriteStdinInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin(TOOL_WRITE_STDIN),
            "Write stdin to, or poll, a live process previously started by exec. Pass empty chars to wait without sending input. Does not start a new command or re-request command approval.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::Process)
    }

    fn execute(
        &self,
        stdin_input: WriteStdinInput,
        _context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let chars = stdin_input.chars.unwrap_or_default();
            let yield_time = if chars.is_empty() {
                poll_yield_duration(stdin_input.yield_time_ms)
            } else {
                yield_duration(stdin_input.yield_time_ms)
            };
            let snapshot = self
                .process_manager
                .write_stdin(CommandWriteRequest {
                    process_id: stdin_input.process_id,
                    chars,
                    yield_time,
                    max_output_chars: max_output_chars(
                        stdin_input.max_output_chars,
                        TruncationStrategy::default()
                            .head_limit
                            .saturating_add(TruncationStrategy::default().tail_limit),
                    ),
                })
                .await?;

            tool_output_from_snapshot(snapshot, TOOL_WRITE_STDIN)
        }
    }
}

fn yield_duration(value: Option<u64>) -> Duration {
    let millis = match value {
        Some(0) => 0,
        Some(value) => value.clamp(MIN_YIELD_TIME_MS, MAX_YIELD_TIME_MS),
        None => DEFAULT_YIELD_TIME_MS,
    };
    Duration::from_millis(millis)
}

fn poll_yield_duration(value: Option<u64>) -> Duration {
    let millis = match value {
        Some(0) => 0,
        Some(value) => value.clamp(DEFAULT_YIELD_TIME_MS, MAX_YIELD_TIME_MS),
        None => DEFAULT_YIELD_TIME_MS,
    };
    Duration::from_millis(millis)
}

fn max_output_chars(value: Option<usize>, default: usize) -> usize {
    value.unwrap_or(default).clamp(1, MAX_MODEL_OUTPUT_CHARS)
}

fn tool_output_from_snapshot(
    snapshot: CommandOutputSnapshot,
    tool: &str,
) -> Result<ToolResult, PureError> {
    let capture_file = snapshot.capture_file.clone();
    let exit_code = snapshot.state.exit_code();
    let timed_out = snapshot.state.is_timed_out();
    let output = CommandJsonOutput {
        state: snapshot.state,
        process_id: snapshot.process_id,
        stdout: snapshot.stdout.content.clone(),
        stderr: snapshot.stderr.content.clone(),
        output_file: snapshot.output_file.display().to_string(),
        output_artifacts: snapshot.output_artifacts.clone(),
        message: snapshot.message,
    };
    let description =
        serde_json::to_string(&output).map_err(|error| PureError::ToolExecutionFailed {
            tool: tool.to_string(),
            error: format!("failed to serialize command output: {error}"),
        })?;
    let mut runtime_events = Vec::new();
    if !snapshot.output_artifacts.is_empty() {
        runtime_events.push(ToolDirective::OutputArtifacts {
            artifacts: snapshot.output_artifacts,
        });
    }
    Ok(ToolResult::from_runtime_text(
        description,
        OutputTruncation {
            stdout: snapshot.stdout,
            stderr: snapshot.stderr,
        },
        capture_file,
        exit_code,
        timed_out,
        runtime_events,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{StaticToolTestExt, ToolInput};
    use crate::{
        CommandCaptureStream, CommandOutputSizes, CommandOutputTarget, CommandSpawnRequest,
        ManagedCommand,
    };
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tool_input(command: &str, _session_id: &str, _tool_id: &str) -> ToolInput {
        ToolInput {
            arguments: serde_json::json!({ "command": command }),
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

    fn tool_workspace(root: &std::path::Path) -> ToolWorkspace {
        ToolWorkspace::new(crate::tool::AgentWorkspace::local(root.to_path_buf()))
    }

    fn directory_tool_workspace(root: &std::path::Path) -> ToolWorkspace {
        ToolWorkspace::new(crate::tool::AgentWorkspace::directory(
            root.to_path_buf(),
            Some(vec![root.join("allowed")]),
        ))
    }

    fn test_tool() -> TestExecTool {
        let root = test_root();
        std::fs::create_dir_all(&root).unwrap();
        ExecTool::new(
            Arc::new(LocalCommandBackend::new(root.clone())),
            tool_workspace(&root),
        )
    }

    fn test_tool_with_root() -> (TestExecTool, PathBuf) {
        let root = test_root();
        std::fs::create_dir_all(&root).unwrap();
        let tool = ExecTool::new(
            Arc::new(LocalCommandBackend::new(root.clone())),
            tool_workspace(&root),
        );
        (tool, root)
    }

    #[test]
    fn exec_schema_explains_local_and_ssh_cwd_contracts() {
        let schema = test_tool().input_schema();
        let description = schema["properties"]["cwd"]["description"].as_str().unwrap();
        assert!(description.contains("Use `.` for the workspace root"));
        assert!(description.contains("workspace-relative path"));
        assert!(description.contains("`src`"));
        assert!(description.contains("SSH execution rejects absolute paths"));
        assert!(description.contains("local absolute paths remain subject"));
    }

    #[tokio::test]
    async fn directory_workspace_explicitly_documents_that_shell_can_bypass_writable_paths() {
        let root = test_root();
        std::fs::create_dir_all(root.join("allowed")).unwrap();
        let tool = ExecTool::new(
            Arc::new(LocalCommandBackend::new(root.clone())),
            directory_tool_workspace(&root),
        );
        let command = if cfg!(target_os = "windows") {
            "Set-Content -Path bypassed.txt -Value bypassed"
        } else {
            "printf bypassed > bypassed.txt"
        };

        let output = tool
            .execute_raw(tool_input(command, "session", "tool"), test_context())
            .await
            .unwrap();
        let (_output, result) = await_successful_command(&tool, output).await;

        assert_eq!(result.state.exit_code(), Some(0));
        assert!(root.join("bypassed.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn shared_tools() -> (TestExecTool, TestWriteStdinTool) {
        let root = test_root();
        std::fs::create_dir_all(&root).unwrap();
        command_tool_pair(
            Arc::new(LocalCommandBackend::new(root.clone())),
            tool_workspace(&root),
        )
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

        assert!(!resolved.starts_with(r"\\?\"));
        std::fs::remove_dir_all(root).unwrap();
    }

    impl CommandBackend for HostedContractBackend {
        type Error = PureError;

        async fn resolve_cwd(
            &self,
            cwd: Option<&std::path::Path>,
            allow_workspace_escape: bool,
        ) -> Result<String, Self::Error> {
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

        async fn spawn(&self, request: CommandSpawnRequest) -> Result<ManagedCommand, Self::Error> {
            self.local.spawn(request).await
        }

        async fn prepare_output(
            &self,
            target: &CommandOutputTarget,
            command: &str,
            working_directory: &str,
        ) -> Result<(), Self::Error> {
            self.local
                .prepare_output(target, command, working_directory)
                .await
        }

        async fn append_output_chunk(
            &self,
            target: &CommandOutputTarget,
            stream: CommandCaptureStream,
            chunk: &[u8],
        ) -> Result<(), Self::Error> {
            self.local.append_output_chunk(target, stream, chunk).await
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

    fn command_json(output: &ToolResult) -> CommandJsonOutput {
        serde_json::from_str(&output.canonical_output()).unwrap()
    }

    async fn await_successful_command<B>(
        tool: &ExecTool<B>,
        mut output: ToolResult,
    ) -> (ToolResult, CommandJsonOutput)
    where
        B: CommandBackend,
    {
        let stdin = WriteStdinTool::new(tool.process_manager.clone());
        let mut result = command_json(&output);
        let mut stdout = result.stdout.clone();
        let mut stderr = result.stderr.clone();
        for _ in 0..6 {
            if result.state.final_result().is_some() {
                break;
            }
            let process_id = result
                .process_id
                .clone()
                .expect("non-final command must remain pollable");
            output = stdin
                .execute_raw(
                    ToolInput {
                        arguments: serde_json::json!({
                            "processId": process_id,
                            "yieldTimeMs": 10_000,
                        }),
                    },
                    test_context(),
                )
                .await
                .unwrap();
            result = command_json(&output);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
        }
        result.stdout = stdout;
        result.stderr = stderr;
        assert!(
            matches!(
                result.state.final_result(),
                Some(CommandProcessFinalResult::Succeeded { .. })
            ),
            "{result:?}"
        );
        (output, result)
    }

    fn test_context() -> ToolCallContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        test_context_with_sender(event_tx)
    }

    fn test_context_with_sender(event_tx: pl_trace::AgentEventSender) -> ToolCallContext {
        ToolCallContext::test(event_tx)
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
        let context = ToolCallContext::new(
            crate::tool::ToolCallIdentity {
                item_id: "tool-1".to_string(),
                turn_id: "turn-1".to_string(),
                ..crate::tool::ToolCallIdentity::default()
            },
            event_tx.clone(),
        );
        let observer = ToolResultOutputObserver {
            emitter: context.output_delta_emitter(),
        };
        drop(context);

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

    #[test]
    fn output_observer_publishes_each_stdout_and_stderr_chunk_to_trace_sink() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 11));
        let started_at = crate::time::unix_seconds();
        let mut item = pl_trace::TracePart::started_tool(
            "turn-1".to_string(),
            "tool-1".to_string(),
            11,
            started_at,
            pl_trace::TraceToolInvocation::new(
                "tool-1".to_string(),
                "exec".to_string(),
                "{}".to_string(),
            ),
        );
        pl_trace::TraceEventSink::emit(
            sink.as_ref(),
            pl_trace::TraceEventDraft::start(
                started_at,
                item.turn_id().to_owned(),
                item.item_id().to_owned(),
                item.source(),
                item.state().clone(),
            ),
        )
        .expect("tool start must seed the canonical lifecycle");
        item.apply(item.command(
            started_at,
            pl_trace::TracePartAction::EnterToolPhase {
                phase: pl_trace::TraceToolActivePhase::Running,
            },
        ))
        .expect("tool must enter its running phase");
        pl_trace::TraceEventSink::emit(
            sink.as_ref(),
            pl_trace::TraceEventDraft::apply(
                started_at,
                item.turn_id().to_owned(),
                item.item_id().to_owned(),
                pl_trace::TracePartAction::EnterToolPhase {
                    phase: pl_trace::TraceToolActivePhase::Running,
                },
            ),
        )
        .expect("running tool snapshot must reach the canonical sink");
        let context = ToolCallContext::new(
            crate::tool::ToolCallIdentity {
                item_id: "tool-1".to_string(),
                turn_id: "turn-1".to_string(),
                ..crate::tool::ToolCallIdentity::default()
            },
            event_tx,
        )
        .with_trace_sink(Some(sink.clone()));
        let observer = ToolResultOutputObserver {
            emitter: context.output_delta_emitter(),
        };

        observer.output_chunk(CommandOutputStream::Stdout, b"out", 1);
        observer.output_chunk(CommandOutputStream::Stderr, b"err", 2);
        if let Some(error) = context.take_output_delta_error() {
            panic!("output observer must publish each chunk: {error}");
        }

        let deltas = sink
            .events()
            .into_iter()
            .filter_map(|event| match event.kind {
                pl_trace::TraceEventKind::TracePartDelta { event } => Some(event),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].started_sequence, 11);
        assert_eq!(deltas[0].revision, item.revision() + 1);
        assert_eq!(deltas[1].revision, item.revision() + 2);
        assert!(matches!(
            &deltas[0].delta,
            pl_trace::TraceDelta::ToolResult { delta } if delta == "out"
        ));
        assert!(matches!(
            &deltas[1].delta,
            pl_trace::TraceDelta::ToolResult { delta } if delta == "[stderr] err"
        ));
    }

    #[tokio::test]
    async fn injected_backend_keeps_the_exec_result_contract() {
        let root = test_root();
        std::fs::create_dir_all(&root).unwrap();
        let backend = Arc::new(HostedContractBackend {
            local: LocalCommandBackend::new(root.clone()),
            publish_count: AtomicUsize::new(0),
        });
        let tool = ExecTool::new(backend.clone(), tool_workspace(&root));

        let output = tool
            .execute_raw(
                tool_input("echo hosted", "hosted-session", "hosted-tool"),
                test_context(),
            )
            .await
            .unwrap();
        let (output, result) = await_successful_command(&tool, output).await;

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
            ToolDirective::OutputArtifacts { artifacts }
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
        let input = tool_input("echo streaming", "stream-session", "stream-tool");
        let output = tool
            .execute_raw(input, test_context_with_sender(event_tx))
            .await
            .unwrap();

        let (streamed, revision) = collect_tool_result_stream(&mut event_rx);

        assert!(streamed.contains("streaming"));
        assert!(revision > 0);
        assert!(!output.model_output().is_empty());
        assert!(revision > 0);
    }

    #[tokio::test]
    async fn streams_tool_result_delta_for_stderr_output() {
        let tool = test_tool();
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let output = tool
            .execute_raw(
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
        assert!(!output.model_output().is_empty());
        assert!(revision > 0);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_default_shell_executes_powershell_script() {
        let tool = test_tool();
        let output = tool
        .execute_raw(
            tool_input(
                "if ($PSVersionTable.PSVersion.Major -ge 5) { Write-Output 'powershell-ok' }; (Get-Location).Path",
                "ps-session",
                "ps-tool",
            ),
            test_context(),
        )
        .await
        .unwrap();
        let (output, result) = await_successful_command(&tool, output).await;

        assert_eq!(output.exit_code, Some(0));
        assert!(result.stdout.contains("powershell-ok"));
        assert!(result.stdout.lines().count() >= 2);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_powershell_captures_unicode_stdout() {
        let tool = test_tool();
        let output = tool
            .execute_raw(
                tool_input("Write-Output '中文输出'", "unicode-session", "unicode-tool"),
                test_context(),
            )
            .await
            .unwrap();
        let (output, result) = await_successful_command(&tool, output).await;

        assert_eq!(output.exit_code, Some(0));
        assert!(result.stdout.contains("中文输出"));
    }

    #[tokio::test]
    async fn defaults_to_workspace_root_as_current_directory() {
        let (tool, root) = test_tool_with_root();
        let output = tool
            .execute_raw(
                tool_input("echo marker > cwd-check.txt", "cwd-session", "cwd-tool"),
                test_context(),
            )
            .await
            .unwrap();
        let (output, _result) = await_successful_command(&tool, output).await;

        assert_eq!(output.exit_code, Some(0));
        assert!(root.join("cwd-check.txt").exists());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn rejects_working_directory_outside_workspace() {
        let tool = test_tool();
        let result = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": "echo no",
                        "cwd": ".."
                    }),
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
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": "echo marker > cwd-check.txt",
                        "cwd": "subdir",
                    }),
                },
                test_context(),
            )
            .await
            .unwrap();
        let (output, _result) = await_successful_command(&tool, output).await;

        assert_eq!(output.exit_code, Some(0));
        assert!(root.join("subdir/cwd-check.txt").exists());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn full_access_allows_working_directory_outside_workspace() {
        let (tool, root) = test_tool_with_root();
        let outside = root.parent().unwrap().to_path_buf();
        let context = test_context().with_approval(crate::tool::ToolApprovalContext::new(
            crate::turn::PermissionMode::FullAccess,
            crate::tool::WorkspaceAccess::ExternalAllowed,
        ));
        let output = tool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": "echo yes",
                        "cwd": outside,
                    }),
                },
                context,
            )
            .await
            .unwrap();
        let (output, _result) = await_successful_command(&tool, output).await;

        assert_eq!(output.exit_code, Some(0));
        let _ = tokio::fs::remove_file(&output.output_file).await;
        let _ =
            tokio::fs::remove_dir_all(output.output_file.parent().unwrap().parent().unwrap()).await;
    }

    #[tokio::test]
    #[ignore = "requires ripgrep on the host"]
    async fn exec_allows_search_read_and_write_in_read_only_workspace() {
        let root = test_root();
        std::fs::create_dir_all(&root).unwrap();
        let tool = ExecTool::new(
            Arc::new(LocalCommandBackend::new(root.clone())),
            ToolWorkspace::new(crate::tool::AgentWorkspace::confined(
                root.clone(),
                crate::tool::WorkspaceMutability::ReadOnly,
            )),
        );
        tokio::fs::write(root.join("read-only-source.txt"), "read-only fixture\n")
            .await
            .unwrap();
        let context = test_context();

        let search = tool
            .execute_raw(
                tool_input("rg --version", "read-only", "search"),
                context.clone(),
            )
            .await
            .unwrap();
        let (search, search_result) = await_successful_command(&tool, search).await;
        assert_eq!(search.exit_code, Some(0));
        assert!(search_result.stdout.contains("ripgrep"));

        #[cfg(windows)]
        let read_command = "Get-Content -LiteralPath 'read-only-source.txt'";
        #[cfg(not(windows))]
        let read_command = "cat read-only-source.txt";
        let read = tool
            .execute_raw(
                tool_input(read_command, "read-only", "read"),
                context.clone(),
            )
            .await
            .unwrap();
        let (read, read_result) = await_successful_command(&tool, read).await;
        assert_eq!(read.exit_code, Some(0));
        assert!(read_result.stdout.contains("read-only fixture"));

        #[cfg(windows)]
        let write_command = "Set-Content -LiteralPath 'shell-write.txt' -Value 'written'";
        #[cfg(not(windows))]
        let write_command = "printf written > shell-write.txt";
        let write = tool
            .execute_raw(tool_input(write_command, "read-only", "write"), context)
            .await
            .unwrap();
        let (write, _write_result) = await_successful_command(&tool, write).await;
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
            .execute_raw(tool_input("echo test", "s5", "t5"), test_context())
            .await
            .unwrap();
        let (output, _result) = await_successful_command(&tool, output).await;

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
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let context = ToolCallContext::new(
            crate::tool::ToolCallIdentity {
                session_id: "my-session".to_string(),
                item_id: "my-tool".to_string(),
                call_id: "my-call".to_string(),
                ..crate::tool::ToolCallIdentity::default()
            },
            event_tx,
        );
        let output = tool
            .execute_raw(tool_input("echo ok", "my-session", "my-tool"), context)
            .await
            .unwrap();
        let (output, _result) = await_successful_command(&tool, output).await;

        let path = output.output_file;
        assert!(path.ends_with("target/pure/my-session/my-tool/output.log"));

        let _ = tokio::fs::remove_file(&path).await;
        let _ = tokio::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).await;
    }

    #[tokio::test]
    async fn long_command_returns_process_id_then_can_be_polled() {
        let (exec, stdin) = shared_tools();
        let running = exec
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": sleep_then_echo_command(),
                        "yieldTimeMs": 250,
                    }),
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
                .execute_raw(
                    ToolInput {
                        arguments: serde_json::json!({
                            "processId": process_id,
                            "chars": "",
                            "yieldTimeMs": 1500,
                        }),
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
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": stdin_echo_command(),
                        "yieldTimeMs": 250,
                    }),
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
                .execute_raw(
                    ToolInput {
                        arguments: serde_json::json!({
                            "processId": process_id,
                            "chars": if attempt == 0 { "hello\n" } else { "" },
                            "yieldTimeMs": 3000,
                        }),
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
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": long_sleep_then_echo_command(),
                        "timeoutSeconds": 1,
                        "yieldTimeMs": 3000,
                    }),
                },
                test_context(),
            )
            .await
            .unwrap();
        let mut result = command_json(&output);
        let mut timed_out = output.timed_out || result.state.is_timed_out();
        for _attempt in 0..8 {
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
                .execute_raw(
                    ToolInput {
                        arguments: serde_json::json!({
                            "processId": process_id,
                            "yieldTimeMs": 1000,
                        }),
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
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": sleep_then_echo_command(),
                        "yieldTimeMs": 250,
                    }),
                },
                test_context(),
            )
            .await
            .unwrap();
        let process_id = command_json(&first).process_id.unwrap();

        let second = exec
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": sleep_then_echo_command(),
                        "yieldTimeMs": 250,
                    }),
                },
                test_context(),
            )
            .await;

        assert!(second.unwrap_err().to_string().contains("process limit"));

        let _ = stdin
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "processId": process_id,
                        "yieldTimeMs": 3000,
                    }),
                },
                test_context(),
            )
            .await;
    }

    #[tokio::test]
    async fn write_stdin_unknown_process_is_recoverable_error() {
        let (_bash, stdin) = shared_tools();
        let result = stdin
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({ "processId": "missing" }),
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
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": large_output_command(),
                        "maxOutputChars": 100,
                    }),
                },
                test_context(),
            )
            .await
            .unwrap();
        let (output, result) = await_successful_command(&tool, output).await;
        let file_content = tokio::fs::read_to_string(&output.output_file)
            .await
            .unwrap();

        assert!(result.stdout.len() < 5000);
        assert!(result.stdout.contains("omitted"));
        assert!(file_content.len() > result.stdout.len());
    }
}

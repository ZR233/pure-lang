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

/// 向会话命令任务写入 stdin；等待由统一 wait 工具负责。
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
    /// Maximum stdout/stderr characters returned to the model.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub max_output_chars: Option<usize>,
}

/// `write_stdin` 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteStdinInput {
    /// Task id returned by exec's acceptance receipt.
    pub task_id: String,
    /// Nonempty text to write to stdin. This tool does not wait or poll.
    #[schemars(length(min = 1))]
    pub chars: String,
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
    task_id: Option<String>,
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
            "Start a session-owned shell command. The acceptance receipt contains taskId; use wait for final completion or write_stdin with that taskId for input. Completion means the process has exited and output has drained. Commands are not constrained by a directory Profile's writablePaths; obey the frozen assignment. Full output is saved to outputFile.",
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
            let task_id = context.task_id().ok_or_else(|| {
                crate::tool::tool_error(TOOL_EXEC, "exec requires a session task identity")
            })?;
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
                .run_task(
                    task_id,
                    CommandStartRequest {
                        command: exec_input.command,
                        cwd: exec_input.cwd,
                        allow_workspace_escape: self.workspace.allows_workspace_escape(&context),
                        timeout,
                        yield_time: Duration::ZERO,
                        max_output_chars: max_output_chars(
                            exec_input.max_output_chars,
                            self.default_max_output_chars(),
                        ),
                        session_id: context.identity().session_id.clone(),
                        tool_id: context.identity().item_id.clone(),
                        call_id,
                        cancellation_token: context.cancellation_token(),
                        output_observer: Some(observer),
                    },
                )
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
            "Write nonempty input to a live exec task using its taskId. This does not wait, poll, start a new command, or request command approval again. Use wait for completion events.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::control()
            .with_effect(ToolEffect::Process)
            .with_runtime_lock_policy(crate::tool::ToolRuntimeLockPolicy::None)
    }

    fn execute(
        &self,
        stdin_input: WriteStdinInput,
        _context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            if stdin_input.chars.is_empty() {
                return Err(crate::tool::tool_error(
                    TOOL_WRITE_STDIN,
                    "chars must be nonempty; use wait to suspend",
                ));
            }
            let snapshot = self
                .process_manager
                .write_stdin(CommandWriteRequest {
                    process_id: stdin_input.task_id,
                    chars: stdin_input.chars,
                    yield_time: Duration::ZERO,
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
    let failed = match snapshot.state.final_result() {
        Some(CommandProcessFinalResult::Succeeded { .. }) | None => false,
        Some(
            CommandProcessFinalResult::Failed { .. }
            | CommandProcessFinalResult::TimedOut
            | CommandProcessFinalResult::Cancelled,
        ) => true,
    };
    let output = CommandJsonOutput {
        state: snapshot.state,
        task_id: snapshot.process_id,
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
    if failed {
        runtime_events.push(ToolDirective::ExecutionFailed);
    }
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
    use pretty_assertions::assert_eq;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "pure-test-tool-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn exec_schema_explains_local_and_ssh_cwd_contracts() {
        let schema = schemars::schema_for!(ExecInput).to_value();
        let description = schema["properties"]["cwd"]["description"].as_str().unwrap();
        assert!(description.contains("Use `.` for the workspace root"));
        assert!(description.contains("workspace-relative path"));
        assert!(description.contains("`src`"));
        assert!(description.contains("SSH execution rejects absolute paths"));
        assert!(description.contains("local absolute paths remain subject"));
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
    async fn write_stdin_unknown_task_is_recoverable_error() {
        let stdin = WriteStdinTool::new(CommandProcessManager::new(Arc::new(
            LocalCommandBackend::new(std::env::temp_dir()),
        )));
        let (sender, _receiver) = tokio::sync::broadcast::channel(8);
        let result = stdin
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({ "taskId": "missing", "chars": "x" }),
                },
                ToolCallContext::test(sender),
            )
            .await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("has no live process")
        );
    }
}

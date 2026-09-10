//! Command execution through the opaque Thread contract.
use std::{fmt, future::Future, sync::Arc, time::Duration};

use pl_core::context::{ContextContent, OpaquePayload, ResourceReference};
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{DEFAULT_TIMEOUT_SECS, ExecInput, MAX_MODEL_OUTPUT_CHARS};
use crate::command::{
    CommandBackend,
    process_manager::{
        CommandOutputSnapshot, CommandProcessFinalResult, CommandProcessManager,
        CommandStartRequest,
    },
};

/// Host-selected command scope. Dynamic input cannot grant workspace escape.
#[derive(Debug, Clone, Copy)]
pub enum CommandAccess {
    WorkspaceOnly,
    HostGranted,
    /// Host paths require an invocation capability from the registered policy.
    HostApproval,
}

/// Retains complete captured bytes under a stable, readable resource identity.
pub trait CommandOutputArchive: fmt::Debug + Send + Sync + 'static {
    /// Archives output after physical process exit and stream drain.
    ///
    /// # Errors
    /// On failure, keep the source capture intact so the host can retry archiving without
    /// re-executing the command. Never return a temporary path as a durable resource.
    fn retain(
        &self,
        thread_id: &str,
        snapshot: &CommandOutputSnapshot,
    ) -> impl Future<Output = Result<ResourceReference, ToolError>> + Send;
}

/// Thread-owned exec instance using a host-provided backend and output archive.
#[derive(Debug)]
pub struct ThreadExecTool<B: CommandBackend, A: CommandOutputArchive> {
    processes: Arc<CommandProcessManager<B>>,
    archive: Arc<A>,
    access: CommandAccess,
}

impl<B: CommandBackend, A: CommandOutputArchive> ThreadExecTool<B, A> {
    /// Binds services once; the returned tool must belong to only one Thread.
    pub fn new(backend: Arc<B>, archive: Arc<A>, access: CommandAccess) -> Self {
        Self {
            processes: Arc::new(CommandProcessManager::new(backend)),
            archive,
            access,
        }
    }

    /// Rebuilds declarations while retaining this Thread's existing physical command manager.
    pub fn from_process_manager(
        processes: Arc<CommandProcessManager<B>>,
        archive: Arc<A>,
        access: CommandAccess,
    ) -> Self {
        Self {
            processes,
            archive,
            access,
        }
    }

    /// Transfers paired instances over this Thread's one process manager.
    ///
    /// # Errors
    /// Returns invalid declaration identities before registration.
    pub fn registrations(
        self,
        exec: OpaquePayload,
        stdin: OpaquePayload,
    ) -> Result<Vec<Registration>, RegistryError> {
        let input = ThreadWriteStdinTool {
            processes: self.processes.clone(),
        };
        Ok(vec![
            self.registration(exec)?,
            Registration::new(super::TOOL_WRITE_STDIN.into(), stdin, input)?
                .foreground_coexisting(),
        ])
    }

    /// Transfers this instance with the host's frozen model declaration.
    ///
    /// # Errors
    /// Returns an invalid tool identity error before registration.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        Registration::new(super::TOOL_EXEC.into(), declaration, self)
    }
}

/// Input writer paired with an exec instance from the same Thread assembly.
#[derive(Debug)]
struct ThreadWriteStdinTool<B: CommandBackend> {
    processes: Arc<CommandProcessManager<B>>,
}

impl<B: CommandBackend> Tool for ThreadWriteStdinTool<B> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let input: super::WriteStdinInput =
            serde_json::from_str(input.content()).map_err(ToolError::new)?;
        if input.chars.is_empty() || input.max_output_chars == Some(0) {
            return Err(ToolError::new(ExecFailure::InvalidInput));
        }
        let access = context
            .tasks
            .as_ref()
            .ok_or_else(|| ToolError::new(ExecFailure::InvalidInput))?;
        let target = access.get(&input.task_id).map_err(ToolError::new)?;
        if target.tool_id != super::TOOL_EXEC
            || target.status != pl_core::thread::task::TaskStatus::Running
            || target.cancel_requested
        {
            return Err(ToolError::new(ExecFailure::InvalidInput));
        }
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let snapshot = self
            .processes
            .write_stdin(crate::command::process_manager::CommandWriteRequest {
                process_id: safe_identity(&target.call_id),
                chars: input.chars,
                yield_time: Duration::ZERO,
                max_output_chars: input
                    .max_output_chars
                    .unwrap_or(MAX_MODEL_OUTPUT_CHARS)
                    .min(MAX_MODEL_OUTPUT_CHARS),
            })
            .await
            .map_err(ToolError::new)?;
        projection(&snapshot, None)
    }
}

#[derive(Debug, thiserror::Error)]
enum ExecFailure {
    #[error("write_stdin requires nonempty input and an active exec task in this Thread")]
    InvalidInput,
    #[error("exec command must not be empty")]
    EmptyCommand,
    #[error("exec timeoutSeconds and maxOutputChars must be positive")]
    InvalidLimit,
    #[error("command did not complete successfully: {0}")]
    Process(String),
}

impl<B: CommandBackend, A: CommandOutputArchive> Tool for ThreadExecTool<B, A> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let input: ExecInput = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        if input.command.trim().is_empty() {
            return Err(ToolError::new(ExecFailure::EmptyCommand));
        }
        if input.timeout_seconds == Some(0) || input.max_output_chars == Some(0) {
            return Err(ToolError::new(ExecFailure::InvalidLimit));
        }
        let task_id = safe_identity(&context.call_id);
        let (observer, previews) = super::progress::CommandPreview::channel();
        let execution = self.processes.run_task(
            &task_id,
            CommandStartRequest {
                command: input.command,
                cwd: input.cwd,
                allow_workspace_escape: match self.access {
                    CommandAccess::WorkspaceOnly => false,
                    CommandAccess::HostGranted => true,
                    CommandAccess::HostApproval => context
                        .grant
                        .contains(crate::approval::HOST_WORKSPACE_ACCESS),
                },
                timeout: Duration::from_secs(input.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECS)),
                yield_time: Duration::ZERO,
                max_output_chars: input
                    .max_output_chars
                    .unwrap_or(MAX_MODEL_OUTPUT_CHARS)
                    .min(MAX_MODEL_OUTPUT_CHARS),
                session_id: safe_identity(&context.thread_id),
                tool_id: task_id.clone(),
                call_id: task_id.clone(),
                cancellation_token: Some(context.cancellation.clone()),
                output_observer: Some(observer),
            },
        );
        let snapshot = super::progress::drive(execution, previews, context.tasks.as_ref())
            .await
            .map_err(ToolError::new)?;
        let retained = self
            .archive
            .retain(&context.thread_id, &snapshot)
            .await
            .and_then(|resource| {
                resource.validate().map_err(ToolError::new)?;
                Ok(resource)
            });
        let output = projection(&snapshot, retained.as_ref().ok())?;
        if let Err(error) = retained {
            return Err(error.with_output(output));
        }
        match snapshot.state.final_result() {
            Some(CommandProcessFinalResult::Succeeded { .. }) => Ok(output),
            Some(
                CommandProcessFinalResult::Failed { .. }
                | CommandProcessFinalResult::TimedOut
                | CommandProcessFinalResult::Cancelled,
            )
            | None => {
                Err(ToolError::new(ExecFailure::Process(snapshot.message)).with_output(output))
            }
        }
    }
}

// Model-supplied call IDs are not filesystem components.
fn safe_identity(id: &str) -> String {
    let digest = Sha256::digest(id.as_bytes());
    format!("task-{digest:x}")
}

fn projection(
    snapshot: &CommandOutputSnapshot,
    resource: Option<&ResourceReference>,
) -> Result<ToolOutput, ToolError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Record<'a> {
        state: &'a crate::command::process_manager::CommandProcessLifecycle,
        process_id: &'a Option<String>,
        stdout: &'a pl_output::TruncatedOutput,
        stderr: &'a pl_output::TruncatedOutput,
        capture_file: &'a std::path::Path,
        output_file: &'a std::path::Path,
        output_artifacts: &'a [serde_json::Value],
        resource: Option<&'a ResourceReference>,
        output_revision: u64,
        message: &'a str,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Preview<'a> {
        state: &'a crate::command::process_manager::CommandProcessLifecycle,
        stdout: &'a str,
        stderr: &'a str,
        archived: bool,
    }
    let payload = serde_json::to_string(&Record {
        state: &snapshot.state,
        process_id: &snapshot.process_id,
        stdout: &snapshot.stdout,
        stderr: &snapshot.stderr,
        capture_file: &snapshot.capture_file,
        output_file: &snapshot.output_file,
        output_artifacts: &snapshot.output_artifacts,
        resource,
        output_revision: snapshot.output_revision,
        message: &snapshot.message,
    })
    .map_err(ToolError::new)?;
    let preview = serde_json::to_string(&Preview {
        state: &snapshot.state,
        stdout: &snapshot.stdout.content,
        stderr: &snapshot.stderr.content,
        archived: resource.is_some(),
    })
    .map_err(ToolError::new)?;
    let mut content = vec![ContextContent::Text {
        text: Arc::from(preview),
    }];
    if let Some(resource) = resource {
        content.push(ContextContent::Resource {
            reference: resource.clone(),
        });
    }
    Ok(ToolOutput::new(
        OpaquePayload::new("pl.tool.exec", 1, payload).map_err(ToolError::new)?,
        content,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[cfg(unix)]
    #[tokio::test]
    async fn core_call_ids_execute_and_stdin_targets_the_same_process_after_catalog_refresh() {
        use pl_core::{model::*, thread::*};
        const CALL: &str = "provider/call:with unsafe path chars";
        struct Calls {
            next: usize,
        }
        impl ModelSession for Calls {
            async fn prepare(
                &mut self,
                request: ModelRequest,
            ) -> Result<PreparedModelCall, ModelError> {
                let next = self.next;
                self.next += 1;
                Ok(PreparedModelCall::new(async move {
                    Ok(ModelStepOutput {
                        attempt_id: request.attempt_id,
                        base_context_revision: request.context.revision,
                        content: Vec::new(),
                        private_context: None,
                        usage: Default::default(),
                        tool_calls: [
                            ModelToolCall {
                                call_id: CALL.into(),
                                tool_id: "exec".into(),
                                arguments: crate::test_support::input(
                                    serde_json::json!({"command":"printf READY; read -r line; printf '%s' \"$line\"", "timeoutSeconds":10}),
                                ),
                            },
                            ModelToolCall {
                                call_id: "stdin".into(),
                                tool_id: "write_stdin".into(),
                                arguments: crate::test_support::input(
                                    serde_json::json!({"taskId":format!("task:{CALL}"),"chars":"STDIN_REACHED_PROCESS\n"}),
                                ),
                            },
                        ].into_iter().skip(next).take(1).collect(),
                    })
                }))
            }
            async fn close(&mut self) -> Result<(), ModelError> {
                Ok(())
            }
        }
        #[derive(Debug)]
        struct Archive;
        impl CommandOutputArchive for Archive {
            async fn retain(
                &self,
                _: &str,
                snapshot: &CommandOutputSnapshot,
            ) -> Result<ResourceReference, ToolError> {
                let bytes = tokio::fs::read(&snapshot.capture_file)
                    .await
                    .map_err(ToolError::new)?;
                ResourceReference::new(
                    "saved-command".into(),
                    format!("sha256:{:x}", Sha256::digest(&bytes)),
                    bytes.len() as u64,
                    "text/plain".into(),
                )
                .map_err(ToolError::new)
            }
        }
        let root = tempfile::tempdir().unwrap();
        let manager = Arc::new(CommandProcessManager::new(Arc::new(
            crate::command::LocalCommandBackend::new(root.path()),
        )));
        let registrations = || {
            ThreadExecTool::from_process_manager(
                manager.clone(),
                Arc::new(Archive),
                CommandAccess::WorkspaceOnly,
            )
            .registrations(OpaquePayload::text("exec"), OpaquePayload::text("stdin"))
            .unwrap()
        };
        let thread = ThreadHandle::start(
            "thread/unsafe:identity".into(),
            DynModelSession::new(Calls { next: 0 }),
        )
        .unwrap();
        thread.register_tools(registrations()).await.unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let running = tokio::spawn({
            let thread = thread.clone();
            async move { thread.execute_tool(CALL.into(), Default::default()).await }
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if thread.snapshot().tool_progress.values().flatten().any(|content| matches!(content, ContextContent::Text { text } if text.contains("READY"))) { break; }
                assert!(!running.is_finished(), "exec failed before publishing READY");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.unwrap();
        thread.register_tools(registrations()).await.unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "stdin-attempt".into(),
                content: Vec::new(),
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        thread
            .execute_tool("stdin".into(), Default::default())
            .await
            .unwrap();
        running.await.unwrap().unwrap();
        thread
            .wait_task(&format!("task:{CALL}"), Default::default())
            .await
            .unwrap();
        let state = thread.snapshot();
        assert!(
            state
                .tasks
                .values()
                .all(|task| task.status == task::TaskStatus::Succeeded),
            "tasks={:?}; deliveries={:?}",
            state.tasks,
            state.deliveries
        );
        let delivered = state
            .deliveries
            .iter()
            .find(|delivery| delivery.call_id == CALL)
            .unwrap();
        assert!(
            delivered
                .output
                .payload()
                .content()
                .contains("STDIN_REACHED_PROCESS")
        );
        let id = safe_identity(CALL);
        assert!(id.starts_with("task-") && id.len() < 256 && !id.contains('/'));
        assert_ne!(id, safe_identity("another call"));
        thread.close().await.unwrap();
    }

    #[test]
    fn output_projection_keeps_capture_facts_out_of_model_text_and_preserves_archive_identity() {
        let snapshot = CommandOutputSnapshot {
            state: serde_json::from_value(serde_json::json!({
                "kind": "final", "data": {"result": {"kind": "succeeded", "data": {"exit_code": 0}}},
            })).unwrap(),
            process_id: Some("process".into()),
            stdout: pl_output::TruncatedOutput { content: "preview".into(), was_truncated: true, original_length: 1000 },
            stderr: pl_output::TruncatedOutput::empty(),
            output_file: "/private/output".into(), capture_file: "/private/capture".into(),
            message: "done".into(), output_revision: 4, output_artifacts: Vec::new(),
        };
        let resource = ResourceReference::new(
            "stable-output".into(),
            format!("sha256:{:x}", Sha256::digest(b"complete output")),
            15,
            "text/plain".into(),
        )
        .unwrap();
        let output = projection(&snapshot, Some(&resource)).unwrap();
        assert_eq!(
            output.context()[1],
            ContextContent::Resource {
                reference: resource
            }
        );
        let ContextContent::Text { text } = &output.context()[0] else {
            panic!("expected preview")
        };
        assert!(!text.contains("/private/"));
        let payload: serde_json::Value = serde_json::from_str(output.payload().content()).unwrap();
        assert_eq!(payload["captureFile"], "/private/capture");
        assert_eq!(payload["stdout"]["originalLength"], 1000);
        let failed_archive = projection(&snapshot, None).unwrap();
        assert_eq!(failed_archive.context().len(), 1);
        assert!(
            failed_archive
                .payload()
                .content()
                .contains("/private/capture")
        );
    }
}

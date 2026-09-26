//! Command execution through the opaque Thread contract.
use std::{fmt, future::Future, sync::Arc, time::Duration};

use pl_core::context::{ContextContent, OpaquePayload, ResourceReference};
use pl_core::thread::cold::{
    ColdStoreError, OutputRepair, OutputRetryFuture, OutputRetryObligation, OutputRetryOutcome,
    OutputStorageFault, StorageFaultKind,
};
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
        CaptureRepair, CommandCaptureFailure, CommandOutputSnapshot, CommandProcessFinalResult,
        CommandProcessManager, CommandStartRequest,
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

/// Typed obligation that re-saves a command capture the archive could not store.
///
/// It first re-materializes any accepted bytes a failed capture append could not write — truncating
/// the fragment back to the offset it was at before the failed append and re-appending exactly that
/// chunk — so a retry never archives a short fragment as if it were whole. It then re-runs the *same*
/// archive call with the *same* capture snapshot, so retrying re-saves the exact accepted bytes under
/// their stable content-addressed identity: it never reruns the command, and a repeated retry
/// produces the same reference instead of a duplicate. A successful retry also names the committed
/// tool result — by the same call id it was committed under — so the durable reference reaches that
/// result instead of becoming an orphan the UI and cold recovery cannot locate.
#[derive(Debug)]
struct ArchiveRetentionObligation<A: CommandOutputArchive, B: CommandBackend> {
    archive: Arc<A>,
    backend: Arc<B>,
    thread_id: String,
    call_id: String,
    snapshot: CommandOutputSnapshot,
    repair: Option<CaptureRepair>,
    kind: StorageFaultKind,
}

impl<A: CommandOutputArchive, B: CommandBackend> ArchiveRetentionObligation<A, B> {
    fn new(
        archive: Arc<A>,
        backend: Arc<B>,
        thread_id: &str,
        call_id: &str,
        snapshot: CommandOutputSnapshot,
        kind: StorageFaultKind,
    ) -> Self {
        // The repair is taken from the snapshot itself: it names exactly the accepted bytes the failed
        // append could not write, at the offset the fragment was at before it tried.
        let repair = snapshot.capture_repair().cloned();
        Self {
            archive,
            backend,
            thread_id: thread_id.to_owned(),
            call_id: call_id.to_owned(),
            snapshot,
            repair,
            kind,
        }
    }
}

impl<A: CommandOutputArchive, B: CommandBackend> OutputRetryObligation
    for ArchiveRetentionObligation<A, B>
{
    fn identity(&self) -> String {
        // The capture path is the stable identity of one operation's accepted output: the live report
        // and the return path both name the same fragment, so the owner attaches one obligation even
        // when the failure is reported twice, and two parallel operations (different paths) keep
        // separate obligations instead of one overwriting the other.
        self.snapshot.capture_file.display().to_string()
    }
    fn retry(&self) -> OutputRetryFuture<'_> {
        Box::pin(async move {
            // Re-materialize every accepted chunk the failed capture did not store, in acceptance
            // order, before archiving: one truncation back to the confirmed offset, then each chunk
            // re-appended with its own framing. So the archive stores exactly the bytes that were
            // accepted — including the other stream's chunk the first failing append never saw —
            // instead of a short fragment.
            if let Some(repair) = &self.repair {
                let mut committed_len = repair.committed_len;
                for chunk in &repair.chunks {
                    committed_len = self
                        .backend
                        .repair_output_chunk(
                            &self.snapshot.capture_file,
                            chunk.stream,
                            committed_len,
                            &chunk.pending,
                        )
                        .await
                        .map_err(|error| ColdStoreError {
                            source: Box::new(std::io::Error::other(error.to_string())),
                        })?;
                }
            }
            let reference = self
                .archive
                .retain(&self.thread_id, &self.snapshot)
                .await
                .map_err(|error| ColdStoreError {
                    source: Box::new(error),
                })?;
            // The stored reference belongs to the already-committed tool result of this same call:
            // name that identity so the owner attaches the reference to it instead of leaving an
            // orphan blob the UI and cold recovery can never locate.
            Ok(OutputRetryOutcome::StoredWithRepair(OutputRepair {
                call_id: self.call_id.clone(),
                reference,
            }))
        })
    }

    fn received_bytes(&self) -> u64 {
        // The bytes this operation really accepted: the fragment the backend last confirmed writing,
        // plus every accepted chunk still owed. Both are facts the obligation already carries, so the
        // reported size never depends on a fragment-length read that could fail and understate it.
        let committed = self.snapshot.capture_committed_len;
        let pending: u64 = self
            .snapshot
            .capture_repair()
            .map(|repair| {
                repair
                    .chunks
                    .iter()
                    .map(|chunk| chunk.pending.len() as u64)
                    .sum()
            })
            .unwrap_or(0);
        committed.saturating_add(pending)
    }

    fn location(&self) -> String {
        self.snapshot.capture_file.display().to_string()
    }

    fn kind(&self) -> StorageFaultKind {
        self.kind
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
            return Err(ToolError::new(ExecFailure::TaskUnavailable {
                task_id: input.task_id,
                status: target.status,
                cancelled: target.cancel_requested,
            }));
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
    #[error(
        "stdin task {task_id} is not a running exec task: status={status:?}, cancelRequested={cancelled}"
    )]
    TaskUnavailable {
        task_id: String,
        status: pl_core::thread::task::TaskStatus,
        cancelled: bool,
    },
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
        let max_output_chars = input
            .max_output_chars
            .unwrap_or(MAX_MODEL_OUTPUT_CHARS)
            .min(MAX_MODEL_OUTPUT_CHARS);
        let (observer, previews, failures) = super::progress::ExecOutputObserver::channel();
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
                max_output_chars,
                session_id: safe_identity(&context.thread_id),
                tool_id: task_id.clone(),
                call_id: task_id.clone(),
                cancellation_token: Some(context.cancellation.clone()),
                output_observer: Some(observer),
            },
        );
        // Report a capture failure through the running call's reliable channel the moment the reader
        // observes it — before the process tree finishes terminating and draining — so other
        // model/tool admission is blocked while this call is still in flight, not only when `drive`
        // returns. The same obligation is reported again on the return path below, so stopping this
        // task here can never lose the report.
        let reporter = tokio::spawn({
            let processes = self.processes.clone();
            let archive = self.archive.clone();
            let thread_id = context.thread_id.clone();
            let call_id = context.call_id.clone();
            let tasks = context.tasks.clone();
            // The reporter needs its own handle: `task_id` is still borrowed by the running
            // `execution` future below, so it cannot be moved into this task.
            let report_task_id = task_id.clone();
            async move {
                let mut failures = failures;
                while let Some(failure) = failures.recv().await {
                    let Some(snapshot) =
                        processes.snapshot(&report_task_id, max_output_chars).await
                    else {
                        continue;
                    };
                    let fault = capture_output_fault(
                        &failure,
                        archive.clone(),
                        processes.backend(),
                        &thread_id,
                        &call_id,
                        &snapshot,
                    );
                    report_output_storage_fault(tasks.as_ref(), &fault).await;
                }
            }
        });
        let snapshot = super::progress::drive(execution, previews, context.tasks.as_ref())
            .await
            .map_err(ToolError::new)?;
        reporter.abort();
        // Build the typed output fault the moment the capture failed and report it through the running
        // call's reliable channel *before* the archive and projection, so other model/tool admission is
        // blocked immediately instead of only when this call unwinds. A write or read failure keeps its
        // retriable obligation (re-save the accepted capture); a bounded-capture truncation does not,
        // because its accepted bytes are retained in the fragment and the reliable writer saves them.
        let capture_fault = snapshot.output_failure.clone().map(|failure| {
            capture_output_fault(
                &failure,
                self.archive.clone(),
                self.processes.backend(),
                &context.thread_id,
                &context.call_id,
                &snapshot,
            )
        });
        if let Some(fault) = &capture_fault {
            report_output_storage_fault(context.tasks.as_ref(), fault).await;
        }
        let retained = self.retain_capture(&context.thread_id, &snapshot).await;
        let output = projection(&snapshot, retained.as_ref().ok())?;
        if let Err(error) = retained {
            // The archive itself could not store the capture: keep the retriable obligation on the
            // typed fault and report it through the running call's reliable channel as well, so the
            // pause is in force before this call returns.
            let error = attach_archive_obligation(
                error,
                self.archive.clone(),
                self.processes.backend(),
                &context.thread_id,
                &context.call_id,
                &snapshot,
            );
            if let Some(fault) = error
                .source
                .downcast_ref::<OutputStorageFault>()
                .map(|fault| Arc::new(fault.clone()))
            {
                report_output_storage_fault(context.tasks.as_ref(), &fault).await;
            }
            return Err(error.with_output(output));
        }
        match snapshot.state.final_result() {
            // A user cancellation wins over a capture failure: the process was stopped on purpose, so
            // the result stays cancelled even if a concurrent capture write also failed.
            Some(CommandProcessFinalResult::Cancelled) => {
                Err(ToolError::new(pl_core::thread::ThreadError::Cancelled).with_output(output))
            }
            // A capture failure outranks a healthy exit. The process may have exited before its last
            // capture write or flush failed, so checking the typed reason (not only `Failed`) keeps a
            // truncated capture from being reported as success.
            _ if capture_fault.is_some() => {
                // The command's durable capture could not continue. Report it through the typed
                // storage boundary core owns instead of as a plain tool error, so the Thread latches
                // the exact category and pauses further admission; the bytes already captured ride
                // along as the observed output.
                let fault = capture_fault.expect("checked to be present");
                Err(ToolError::new((*fault).clone()).with_output(output))
            }
            Some(CommandProcessFinalResult::Succeeded { .. }) => Ok(output),
            Some(
                CommandProcessFinalResult::Failed { .. } | CommandProcessFinalResult::TimedOut,
            )
            | None => {
                Err(ToolError::new(ExecFailure::Process(snapshot.message)).with_output(output))
            }
        }
    }
}

impl<B: CommandBackend, A: CommandOutputArchive> ThreadExecTool<B, A> {
    /// Archives the accepted capture, re-materializing any bytes the failed capture did not store.
    ///
    /// A failed capture append may have left the fragment short of the bytes already accepted and
    /// published live, and the other stream's reader may already have accepted a following chunk.
    /// Repairing first — truncating back to the backend-confirmed offset and re-appending every
    /// accepted chunk in order through the same backend — makes the archive store the exact accepted
    /// bytes under their stable identity instead of a short fragment it would report as `Stored`. A
    /// repair that itself cannot write is reported as a typed output-storage fault, so the obligation
    /// (which repeats the whole repair) stays owed and the short fragment is never archived as whole.
    async fn retain_capture(
        &self,
        thread_id: &str,
        snapshot: &CommandOutputSnapshot,
    ) -> Result<ResourceReference, ToolError> {
        if let Some(repair) = snapshot.capture_repair() {
            let mut committed_len = repair.committed_len;
            for chunk in &repair.chunks {
                committed_len = self
                    .processes
                    .backend()
                    .repair_output_chunk(
                        &snapshot.capture_file,
                        chunk.stream,
                        committed_len,
                        &chunk.pending,
                    )
                    .await
                    .map_err(|error| {
                        let source = ColdStoreError {
                            source: Box::new(std::io::Error::other(format!(
                                "command output capture repair failed: {error}"
                            ))),
                        };
                        ToolError::new(OutputStorageFault::new(
                            StorageFaultKind::WriteFailed,
                            Arc::new(source),
                        ))
                    })?;
            }
        }
        let reference = self.archive.retain(thread_id, snapshot).await?;
        reference.validate().map_err(ToolError::new)?;
        Ok(reference)
    }
}

/// Builds the typed storage fault for one capture failure, with its retriable obligation when owed.
///
/// The category travels as a value, so core never parses the diagnostic text to classify it. A
/// bounded-capture truncation with every accepted chunk on disk keeps no obligation: its accepted
/// bytes are retained in the fragment and the reliable pause releases once the durability fence lands.
/// A write or read failure, or any failure that left accepted chunks the capture did not store, carries
/// the obligation that re-materializes the exact plan — so a fault that also kept a repair is never
/// classified as a plain budget stop that could archive a short fragment as whole.
fn capture_output_fault<A: CommandOutputArchive, B: CommandBackend>(
    failure: &CommandCaptureFailure,
    archive: Arc<A>,
    backend: Arc<B>,
    thread_id: &str,
    call_id: &str,
    snapshot: &CommandOutputSnapshot,
) -> Arc<OutputStorageFault> {
    let owes_repair = snapshot.capture_repair().is_some();
    let kind = match failure {
        CommandCaptureFailure::Exhausted { .. } if !owes_repair => StorageFaultKind::QueueFull,
        _ => StorageFaultKind::WriteFailed,
    };
    let source = ColdStoreError {
        source: Box::new(std::io::Error::other(failure.message())),
    };
    let fault = OutputStorageFault::new(kind, Arc::new(source));
    if matches!(failure, CommandCaptureFailure::Exhausted { .. }) && !owes_repair {
        Arc::new(fault)
    } else {
        Arc::new(
            fault.with_obligation(Arc::new(ArchiveRetentionObligation::new(
                archive,
                backend,
                thread_id,
                call_id,
                snapshot.clone(),
                kind,
            ))),
        )
    }
}

/// Attaches the archive's retriable obligation to an archive failure that named it.
///
/// Only a failure that already arrived as a typed [`OutputStorageFault`] is rebuilt with the
/// obligation; any other archive error (invalid media, integrity, a plain policy refusal) is returned
/// unchanged, because it is not a reliable-output storage obligation the owner could retry.
fn attach_archive_obligation<A: CommandOutputArchive, B: CommandBackend>(
    error: ToolError,
    archive: Arc<A>,
    backend: Arc<B>,
    thread_id: &str,
    call_id: &str,
    snapshot: &CommandOutputSnapshot,
) -> ToolError {
    let Some(fault) = error.source.downcast_ref::<OutputStorageFault>() else {
        return error;
    };
    let kind = fault.kind;
    let source = fault.source.clone();
    let obligation: Arc<dyn OutputRetryObligation> = Arc::new(ArchiveRetentionObligation::new(
        archive,
        backend,
        thread_id,
        call_id,
        snapshot.clone(),
        kind,
    ));
    ToolError::new(OutputStorageFault::new(kind, source).with_obligation(obligation))
}

/// Reports one typed output fault through the running call's reliable channel, if it has one.
async fn report_output_storage_fault(
    tasks: Option<&pl_core::thread::TaskAccess>,
    fault: &Arc<OutputStorageFault>,
) {
    if let Some(tasks) = tasks {
        // A closed owner or a revoked executor still blocks admission through the return path below,
        // so a refused notification is not the only report; but it must never be silently dropped, so
        // the exact reason is surfaced instead of being discarded.
        if let Err(error) = tasks.report_output_storage_fault(fault.clone()).await {
            tracing::warn!(%error, "the running call could not report its output storage fault early");
        }
    }
}

// Model-supplied call IDs are not filesystem components.
fn safe_identity(id: &str) -> String {
    let digest = Sha256::digest(id.as_bytes());
    format!("task-{}", hex::encode(digest))
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

//! Command process lifecycle and exact terminal result.

mod draining;
mod final_state;
mod running;
mod terminating;

pub use draining::DrainingCommandProcess;
pub use final_state::FinalCommandProcess;
pub use running::RunningCommandProcess;
pub use terminating::TerminatingCommandProcess;

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{
    CommandProcessState, HeadTailBuffer, INTERNAL_BUFFER_BYTES, MAX_CAPTURE_BYTES, StreamKind,
};
use crate::command::CommandCaptureStream;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum CommandProcessLifecycle {
    Running(RunningCommandProcess),
    Terminating(TerminatingCommandProcess),
    Draining(DrainingCommandProcess),
    Final(FinalCommandProcess),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum CommandProcessFinalResult {
    Succeeded { exit_code: i32 },
    Failed { failure: CommandProcessFailure },
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum CommandProcessFailure {
    Exited {
        exit_code: Option<i32>,
    },
    Wait {
        message: String,
    },
    Output {
        message: String,
        exit_code: Option<i32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(super) enum CommandProcessHealth {
    Healthy,
    OutputFailed { message: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CommandTerminationReason {
    TimedOut,
    Cancelled,
    /// The operation's durable output capture could not continue.
    ///
    /// Either the bounded capture capacity was exhausted or a capture write failed; the process is
    /// terminated, the bytes already written stay on disk, and the failure is reported as an output
    /// failure rather than a user cancellation.
    OutputFailed,
}

/// One accepted capture chunk a failed capture has not stored yet, in acceptance order.
///
/// It keeps the stream the bytes came from, so the repair restores the same framing, and the exact
/// accepted bytes, so a retry re-materializes them verbatim instead of re-running the command. The
/// bytes are shared with the reader that accepted them, so retaining the plan never copies output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRepairChunk {
    /// Stream the pending bytes came from, so the repair restores the same framing.
    pub stream: CommandCaptureStream,
    /// The accepted chunk the failed capture could not store.
    pub pending: Arc<[u8]>,
}

/// The exact accepted capture bytes a failed capture left off the fragment, with its committed offset.
///
/// A capture write that fails may leave the fragment short of the bytes already accepted and
/// published live, and the *other* stream's reader may already have accepted a following chunk. This
/// names the offset the backend last confirmed — the fragment is truncated back to it — and every
/// accepted chunk the capture did not store, in acceptance order, so one retry re-materializes exactly
/// those bytes, idempotently and without ever re-running the command. The plan is bounded by the
/// operation's shared capture budget, never by the accumulated output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRepair {
    /// Length of the capture fragment the backend last confirmed: truncate back here before replaying.
    pub committed_len: u64,
    /// Accepted chunks the failed capture did not store, in the order they were accepted.
    pub chunks: Vec<CaptureRepairChunk>,
}

/// Why a running command's durable output capture could not continue.
///
/// The category is a value the reader already has, so a caller reporting the failure to a storage
/// boundary never has to parse the diagnostic text to decide what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandCaptureFailure {
    /// The operation's shared stdout/stderr capture budget was exhausted.
    Exhausted { limit: u64 },
    /// A capture write or flush to disk failed.
    ///
    /// The accepted bytes the failed append could not store are named by the snapshot's
    /// [`CaptureRepair`], so a retry re-materializes exactly those bytes instead of archiving a short
    /// fragment as if it were whole.
    Write { message: String },
    /// The operation's own output stream could not be read any further.
    ///
    /// A read failure is not a healthy end of output: the capture is truncated at this point, so the
    /// operation is stopped and reported with the bytes already accepted instead of being treated as a
    /// clean exit. It is typed like a write failure because it is the same "accepted output could not
    /// be captured" fact, and the retained capture can be re-saved through the same obligation.
    Read { message: String },
}

impl CommandCaptureFailure {
    /// Human-readable reason surfaced in the command result.
    pub fn message(&self) -> String {
        match self {
            Self::Exhausted { limit } => {
                format!("command output capture exceeded the {limit}-byte operation budget")
            }
            Self::Write { message } => format!("command output capture failed: {message}"),
            Self::Read { message } => format!("command output stream could not be read: {message}"),
        }
    }
}

/// Decision the operation's shared capture plan makes for one read chunk.
///
/// Accepting is the single serialization point for both streams: a chunk joins the bounded plan in
/// the exact order it was read, before it is published, so anything the live observer saw is retained
/// even if the other stream's append fails first. A chunk read after the first fault, or one that
/// would exceed the shared budget, is refused instead of joining the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CaptureAcceptance {
    /// The chunk joined the bounded plan and is now published and queued for the single writer.
    Accepted,
    /// The operation's shared capture budget is exhausted; the chunk is refused.
    Exhausted,
    /// A capture fault already stopped admission; the chunk is refused.
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CommandProcessTransition {
    TimeOut,
    Cancel,
    /// The durable output capture could not continue; terminate the operation but keep its bytes.
    OutputFailed {
        failure: CommandCaptureFailure,
    },
    ProcessExited {
        exit_code: Option<i32>,
    },
    ProcessWaitFailed {
        error: String,
    },
    StreamClosed(StreamKind),
}

impl CommandProcessLifecycle {
    pub fn final_result(&self) -> Option<&CommandProcessFinalResult> {
        match self {
            Self::Final(state) => Some(state.result()),
            Self::Running(_) | Self::Terminating(_) | Self::Draining(_) => None,
        }
    }

    pub fn exit_code(&self) -> Option<i32> {
        match self.final_result() {
            Some(CommandProcessFinalResult::Succeeded { exit_code }) => Some(*exit_code),
            Some(CommandProcessFinalResult::Failed {
                failure:
                    CommandProcessFailure::Exited { exit_code }
                    | CommandProcessFailure::Output { exit_code, .. },
            }) => *exit_code,
            Some(CommandProcessFinalResult::Failed {
                failure: CommandProcessFailure::Wait { .. },
            })
            | Some(CommandProcessFinalResult::TimedOut | CommandProcessFinalResult::Cancelled)
            | None => None,
        }
    }

    pub fn is_timed_out(&self) -> bool {
        matches!(
            self,
            Self::Terminating(state) if state.reason() == CommandTerminationReason::TimedOut
        ) || matches!(
            self,
            Self::Draining(state) if matches!(state.result(), CommandProcessFinalResult::TimedOut)
        ) || matches!(
            self,
            Self::Final(state) if matches!(state.result(), CommandProcessFinalResult::TimedOut)
        )
    }

    pub(super) fn is_final(&self) -> bool {
        matches!(self, Self::Final(_))
    }

    pub(super) fn can_accept_input(&self) -> bool {
        matches!(self, Self::Running(_))
    }
}

impl CommandProcessState {
    /// Builds the running state with the capture fragment the backend already confirmed.
    ///
    /// `capture_committed_len` is that confirmed length (the prepared header before any chunk), so a
    /// later repair truncates back to a fact the backend reported instead of a length read that could
    /// fail and be mistaken for "nothing written".
    pub(super) fn new(stdout_open: bool, stderr_open: bool, capture_committed_len: u64) -> Self {
        Self {
            lifecycle: CommandProcessLifecycle::Running(RunningCommandProcess::healthy()),
            stdout_open,
            stderr_open,
            stdout: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            stderr: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            pending_stdout: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            pending_stderr: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            output_revision: 0,
            capture_bytes: 0,
            capture_committed_len,
            capture_pending: std::collections::VecDeque::new(),
            capture_write_failed: false,
            capture_drained: false,
            output_failure: None,
        }
    }

    pub(super) fn can_accept_input(&self) -> bool {
        self.lifecycle.can_accept_input()
    }

    pub(super) fn record_output(&mut self, stream: StreamKind, chunk: &[u8]) -> u64 {
        self.output_revision = self.output_revision.saturating_add(1);
        match stream {
            StreamKind::Stdout => {
                self.stdout.push_chunk(chunk);
                self.pending_stdout.push_chunk(chunk);
            }
            StreamKind::Stderr => {
                self.stderr.push_chunk(chunk);
                self.pending_stderr.push_chunk(chunk);
            }
        }
        self.output_revision
    }

    /// Accepts one read chunk into the operation's bounded, shared, ordered capture plan.
    ///
    /// The plan is the serialization point for both streams: the chunk joins it in the exact order it
    /// was read, before it is published live, so any byte the observer saw is already retained even if
    /// the other stream's append fails first. A chunk read after the first fault is refused, and a
    /// chunk that would exceed [`MAX_CAPTURE_BYTES`] is refused without joining the plan, so the plan
    /// is bounded by the same budget as the capture file.
    pub(super) fn accept_capture(
        &mut self,
        stream: CommandCaptureStream,
        chunk: &[u8],
    ) -> CaptureAcceptance {
        if self.capture_write_failed || self.output_failure.is_some() {
            return CaptureAcceptance::Faulted;
        }
        let next = self.capture_bytes.saturating_add(chunk.len() as u64);
        if next > MAX_CAPTURE_BYTES {
            return CaptureAcceptance::Exhausted;
        }
        self.capture_bytes = next;
        self.capture_pending.push_back(CaptureRepairChunk {
            stream,
            pending: Arc::from(chunk),
        });
        CaptureAcceptance::Accepted
    }

    /// The accepted chunk at the front of the plan, without removing it.
    pub(super) fn front_capture(&self) -> Option<CaptureRepairChunk> {
        self.capture_pending.front().cloned()
    }

    /// Confirms the front chunk was stored and the fragment is now `committed_len` bytes long.
    pub(super) fn confirm_front_capture(&mut self, committed_len: u64) {
        self.capture_pending.pop_front();
        self.capture_committed_len = committed_len;
    }

    /// Marks that a capture write failed, so no further chunk may be appended.
    pub(super) fn fault_capture_write(&mut self) {
        self.capture_write_failed = true;
    }

    pub(super) fn capture_write_faulted(&self) -> bool {
        self.capture_write_failed
    }

    /// Whether the plan still holds an accepted chunk the writer has not stored yet.
    pub(super) fn capture_pending_is_empty(&self) -> bool {
        self.capture_pending.is_empty()
    }

    /// Records that the operation's single capture writer has drained the plan and stopped.
    ///
    /// The terminal result is only published once this is set, so no snapshot can report the exit as
    /// final while the writer still owes accepted bytes to disk.
    pub(super) fn mark_capture_drained(&mut self) {
        self.capture_drained = true;
    }

    /// The exact accepted bytes a failed capture must re-materialize, in acceptance order.
    ///
    /// `None` means every accepted chunk the backend confirmed is already on disk, so no repair is
    /// owed. `committed_len` is always the backend's last confirmed length: an append that could not
    /// report a length keeps the previous one, so the plan never falls back to a fragment-length read
    /// that could fail and be mistaken for "nothing to repair".
    pub(super) fn capture_repair(&self) -> Option<CaptureRepair> {
        if self.capture_pending.is_empty() {
            return None;
        }
        Some(CaptureRepair {
            committed_len: self.capture_committed_len,
            chunks: self.capture_pending.iter().cloned().collect(),
        })
    }

    /// Records the reason the durable capture could not continue, before the process is terminated.
    ///
    /// It sets the health so a process that exits on its own still reports the typed output failure,
    /// and it keeps the *first* category/reason: a second reader that reaches the boundary after the
    /// first failure only terminates the same way and must not overwrite what ended the operation. The
    /// accepted chunks neither reader stored stay accumulated in the one capture plan instead of
    /// replacing one another.
    pub(super) fn record_capture_failure(&mut self, failure: CommandCaptureFailure) {
        if self.output_failure.is_none() {
            self.output_failure = Some(failure);
        }
        if let Some(first) = &self.output_failure {
            let message = first.message();
            self.record_output_error(message);
        }
    }

    pub(super) fn record_output_error(&mut self, error: String) {
        match &mut self.lifecycle {
            CommandProcessLifecycle::Running(state) => state.record_output_error(error),
            CommandProcessLifecycle::Draining(state) => state.record_output_error(error),
            CommandProcessLifecycle::Final(state) => state.record_output_error(error),
            CommandProcessLifecycle::Terminating(_) => {}
        }
    }

    pub(super) fn is_final(&self) -> bool {
        self.lifecycle.is_final()
    }

    pub(super) fn apply_transition(&mut self, transition: CommandProcessTransition) {
        match transition {
            CommandProcessTransition::TimeOut
                if matches!(self.lifecycle, CommandProcessLifecycle::Running(_)) =>
            {
                self.lifecycle = CommandProcessLifecycle::Terminating(
                    TerminatingCommandProcess::new(CommandTerminationReason::TimedOut),
                );
            }
            CommandProcessTransition::Cancel
                if matches!(self.lifecycle, CommandProcessLifecycle::Running(_)) =>
            {
                self.lifecycle = CommandProcessLifecycle::Terminating(
                    TerminatingCommandProcess::new(CommandTerminationReason::Cancelled),
                );
            }
            CommandProcessTransition::OutputFailed { failure }
                if matches!(self.lifecycle, CommandProcessLifecycle::Running(_)) =>
            {
                // The first failure already named the reason; a later one only terminates the same
                // way. Keep it so the reported category and reason never regress.
                if self.output_failure.is_none() {
                    self.output_failure = Some(failure);
                }
                self.lifecycle = CommandProcessLifecycle::Terminating(
                    TerminatingCommandProcess::new(CommandTerminationReason::OutputFailed),
                );
            }
            CommandProcessTransition::ProcessExited { exit_code } => {
                let result = match &self.lifecycle {
                    CommandProcessLifecycle::Terminating(state) => match state.reason() {
                        CommandTerminationReason::TimedOut => CommandProcessFinalResult::TimedOut,
                        CommandTerminationReason::Cancelled => CommandProcessFinalResult::Cancelled,
                        CommandTerminationReason::OutputFailed => {
                            CommandProcessFinalResult::Failed {
                                failure: CommandProcessFailure::Output {
                                    message: self.output_failure.as_ref().map_or_else(
                                        || "command output could not be captured".to_string(),
                                        CommandCaptureFailure::message,
                                    ),
                                    exit_code,
                                },
                            }
                        }
                    },
                    CommandProcessLifecycle::Running(state) => match state.health() {
                        CommandProcessHealth::Healthy if exit_code == Some(0) => {
                            CommandProcessFinalResult::Succeeded { exit_code: 0 }
                        }
                        CommandProcessHealth::Healthy => CommandProcessFinalResult::Failed {
                            failure: CommandProcessFailure::Exited { exit_code },
                        },
                        CommandProcessHealth::OutputFailed { message } => {
                            CommandProcessFinalResult::Failed {
                                failure: CommandProcessFailure::Output {
                                    message: message.clone(),
                                    exit_code,
                                },
                            }
                        }
                    },
                    CommandProcessLifecycle::Draining(_) | CommandProcessLifecycle::Final(_) => {
                        return;
                    }
                };
                self.finish_or_drain(result);
            }
            CommandProcessTransition::ProcessWaitFailed { error } => {
                let result = match &self.lifecycle {
                    CommandProcessLifecycle::Terminating(_)
                    | CommandProcessLifecycle::Running(_) => CommandProcessFinalResult::Failed {
                        failure: CommandProcessFailure::Wait { message: error },
                    },
                    CommandProcessLifecycle::Draining(_) | CommandProcessLifecycle::Final(_) => {
                        return;
                    }
                };
                self.finish_or_drain(result);
            }
            CommandProcessTransition::StreamClosed(stream) => {
                match stream {
                    StreamKind::Stdout => self.stdout_open = false,
                    StreamKind::Stderr => self.stderr_open = false,
                }
                // The final result is published here only once the capture writer has settled the
                // plan: a stream closing is not enough on its own, because the writer may still owe
                // the accepted bytes to disk.
                if self.capture_drained
                    && self.output_streams_closed()
                    && let CommandProcessLifecycle::Draining(state) = &self.lifecycle
                {
                    self.lifecycle = CommandProcessLifecycle::Final(FinalCommandProcess::new(
                        state.result().clone(),
                    ));
                }
            }
            CommandProcessTransition::TimeOut
            | CommandProcessTransition::Cancel
            | CommandProcessTransition::OutputFailed { .. } => {}
        }
    }

    fn finish_or_drain(&mut self, result: CommandProcessFinalResult) {
        // Draining keeps the operation non-final until the capture writer has settled, so a snapshot
        // never reports the process exit as final while accepted bytes are still owed to disk.
        self.lifecycle = if self.capture_drained && self.output_streams_closed() {
            CommandProcessLifecycle::Final(FinalCommandProcess::new(result))
        } else {
            CommandProcessLifecycle::Draining(DrainingCommandProcess::new(result))
        };
    }

    /// Promotes a drained, stream-closed operation from `Draining` to `Final`.
    ///
    /// The capture writer calls this after it settles the plan, so an operation whose process already
    /// exited (or whose streams closed after the exit) becomes final without waiting for another
    /// transition that may never come.
    pub(super) fn promote_drained_final(&mut self) {
        if !(self.capture_drained && self.output_streams_closed()) {
            return;
        }
        let result = match &self.lifecycle {
            CommandProcessLifecycle::Draining(state) => state.result().clone(),
            CommandProcessLifecycle::Running(_)
            | CommandProcessLifecycle::Terminating(_)
            | CommandProcessLifecycle::Final(_) => return,
        };
        self.lifecycle = CommandProcessLifecycle::Final(FinalCommandProcess::new(result));
    }

    pub(super) fn output_streams_closed(&self) -> bool {
        !self.stdout_open && !self.stderr_open
    }
}

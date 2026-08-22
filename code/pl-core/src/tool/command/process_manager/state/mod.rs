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

use super::{CommandProcessState, HeadTailBuffer, INTERNAL_BUFFER_BYTES, StreamKind};

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CommandProcessTransition {
    TimeOut,
    Cancel,
    ProcessExited { exit_code: Option<i32> },
    ProcessWaitFailed { error: String },
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

    pub(super) fn has_live_child(&self) -> bool {
        matches!(self, Self::Running(_) | Self::Terminating(_))
    }

    pub(super) fn can_accept_input(&self) -> bool {
        matches!(self, Self::Running(_))
    }
}

impl CommandProcessState {
    pub(super) fn new(stdout_open: bool, stderr_open: bool) -> Self {
        Self {
            lifecycle: CommandProcessLifecycle::Running(RunningCommandProcess::healthy()),
            stdout_open,
            stderr_open,
            stdout: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            stderr: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            pending_stdout: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            pending_stderr: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            output_revision: 0,
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

    pub(super) fn has_live_child(&self) -> bool {
        self.lifecycle.has_live_child()
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
            CommandProcessTransition::ProcessExited { exit_code } => {
                let result = match &self.lifecycle {
                    CommandProcessLifecycle::Terminating(state) => match state.reason() {
                        CommandTerminationReason::TimedOut => CommandProcessFinalResult::TimedOut,
                        CommandTerminationReason::Cancelled => CommandProcessFinalResult::Cancelled,
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
                    CommandProcessLifecycle::Terminating(state) => match state.reason() {
                        CommandTerminationReason::TimedOut => CommandProcessFinalResult::TimedOut,
                        CommandTerminationReason::Cancelled => CommandProcessFinalResult::Cancelled,
                    },
                    CommandProcessLifecycle::Running(_) => CommandProcessFinalResult::Failed {
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
                if self.output_streams_closed()
                    && let CommandProcessLifecycle::Draining(state) = &self.lifecycle
                {
                    self.lifecycle = CommandProcessLifecycle::Final(FinalCommandProcess::new(
                        state.result().clone(),
                    ));
                }
            }
            CommandProcessTransition::TimeOut | CommandProcessTransition::Cancel => {}
        }
    }

    fn finish_or_drain(&mut self, result: CommandProcessFinalResult) {
        self.lifecycle = if self.output_streams_closed() {
            CommandProcessLifecycle::Final(FinalCommandProcess::new(result))
        } else {
            CommandProcessLifecycle::Draining(DrainingCommandProcess::new(result))
        };
    }

    fn output_streams_closed(&self) -> bool {
        !self.stdout_open && !self.stderr_open
    }
}

use super::{
    CommandProcessPhase, CommandProcessResult, CommandProcessState, CommandProcessTransition,
    CommandTerminationReason, HeadTailBuffer, INTERNAL_BUFFER_BYTES, StreamKind,
};

impl CommandProcessState {
    pub(super) fn new(stdout_open: bool, stderr_open: bool) -> Self {
        Self {
            phase: CommandProcessPhase::Running,
            exit_code: None,
            stdout_open,
            stderr_open,
            stdout: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            stderr: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
            output_revision: 0,
            error: None,
        }
    }

    pub(super) fn can_accept_input(&self) -> bool {
        matches!(self.phase, CommandProcessPhase::Running)
    }

    pub(super) fn is_final(&self) -> bool {
        matches!(self.phase, CommandProcessPhase::Final(_))
    }

    pub(super) fn has_live_child(&self) -> bool {
        matches!(
            self.phase,
            CommandProcessPhase::Running | CommandProcessPhase::Terminating(_)
        )
    }

    pub(super) fn timed_out(&self) -> bool {
        matches!(
            self.phase,
            CommandProcessPhase::Terminating(CommandTerminationReason::TimedOut)
                | CommandProcessPhase::Draining(CommandProcessResult::TimedOut)
                | CommandProcessPhase::Final(CommandProcessResult::TimedOut)
        )
    }

    pub(super) fn is_draining_output(&self) -> bool {
        matches!(self.phase, CommandProcessPhase::Draining(_))
    }

    pub(super) fn termination_reason(&self) -> Option<CommandTerminationReason> {
        match self.phase {
            CommandProcessPhase::Terminating(reason) => Some(reason),
            CommandProcessPhase::Running
            | CommandProcessPhase::Draining(_)
            | CommandProcessPhase::Final(_) => None,
        }
    }

    pub(super) fn apply_transition(&mut self, transition: CommandProcessTransition) {
        match transition {
            CommandProcessTransition::TimedOut => {
                if matches!(self.phase, CommandProcessPhase::Running) {
                    self.phase =
                        CommandProcessPhase::Terminating(CommandTerminationReason::TimedOut);
                }
            }
            CommandProcessTransition::Interrupted => {
                if matches!(self.phase, CommandProcessPhase::Running) {
                    self.phase =
                        CommandProcessPhase::Terminating(CommandTerminationReason::Interrupted);
                }
            }
            CommandProcessTransition::ProcessExited { exit_code } => {
                self.exit_code = exit_code;
                let result = match self.phase {
                    CommandProcessPhase::Terminating(reason) => reason.into(),
                    CommandProcessPhase::Running
                    | CommandProcessPhase::Draining(_)
                    | CommandProcessPhase::Final(_) => {
                        if self.error.is_some() || exit_code != Some(0) {
                            CommandProcessResult::Failed
                        } else {
                            CommandProcessResult::Completed
                        }
                    }
                };
                self.finish_or_drain(result);
            }
            CommandProcessTransition::ProcessWaitFailed => {
                let result = match self.phase {
                    CommandProcessPhase::Terminating(reason) => reason.into(),
                    CommandProcessPhase::Running
                    | CommandProcessPhase::Draining(_)
                    | CommandProcessPhase::Final(_) => CommandProcessResult::Failed,
                };
                self.finish_or_drain(result);
            }
            CommandProcessTransition::StreamClosed(stream) => {
                match stream {
                    StreamKind::Stdout => self.stdout_open = false,
                    StreamKind::Stderr => self.stderr_open = false,
                }
                if let CommandProcessPhase::Draining(result) = self.phase
                    && self.output_streams_closed()
                {
                    self.phase = CommandProcessPhase::Final(result);
                }
            }
        }
    }

    pub(super) fn record_error(&mut self, error: String) {
        self.error = Some(error);
        self.phase = match self.phase {
            CommandProcessPhase::Draining(CommandProcessResult::Completed) => {
                CommandProcessPhase::Draining(CommandProcessResult::Failed)
            }
            CommandProcessPhase::Final(CommandProcessResult::Completed) => {
                CommandProcessPhase::Final(CommandProcessResult::Failed)
            }
            phase => phase,
        };
    }

    fn finish_or_drain(&mut self, result: CommandProcessResult) {
        self.phase = if self.output_streams_closed() {
            CommandProcessPhase::Final(result)
        } else {
            CommandProcessPhase::Draining(result)
        };
    }

    fn output_streams_closed(&self) -> bool {
        !self.stdout_open && !self.stderr_open
    }
}

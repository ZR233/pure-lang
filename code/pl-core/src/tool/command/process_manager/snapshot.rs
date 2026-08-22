use crate::tool::truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};

use super::{
    CommandOutputSnapshot, CommandProcessFailure, CommandProcessFinalResult,
    CommandProcessLifecycle, CommandProcessState, CommandTerminationReason,
};

pub(super) fn message_for_state(
    state: &CommandProcessState,
    process_id: Option<&str>,
    output_file: &std::path::Path,
) -> String {
    match &state.lifecycle {
        CommandProcessLifecycle::Running(_) => format!(
            "Command is still running. Use write_stdin with processId '{}' to wait, poll, or send input. Read outputFile for complete output.",
            process_id.unwrap_or_default()
        ),
        CommandProcessLifecycle::Terminating(state) => format!(
            "Command {} and termination is in progress. Use write_stdin with processId '{}' and empty chars to wait or poll. Read outputFile for captured output.",
            state.reason().message_fragment(),
            process_id.unwrap_or_default()
        ),
        CommandProcessLifecycle::Draining(_) => format!(
            "Command exited and is draining remaining output. Use write_stdin with processId '{}' and empty chars to wait or poll. Read outputFile for complete output.",
            process_id.unwrap_or_default()
        ),
        CommandProcessLifecycle::Final(state) => match state.result() {
            CommandProcessFinalResult::Succeeded { .. } => {
            "Command completed successfully. Read outputFile for complete output if needed."
                .to_string()
            }
            CommandProcessFinalResult::Failed { failure } => format!(
                "{}. Full command output is available at '{}'.",
                failure_message(failure),
                output_file.display()
            ),
            CommandProcessFinalResult::TimedOut => {
            "Command timed out and was terminated. Read outputFile for captured output.".to_string()
            }
            CommandProcessFinalResult::Cancelled => {
            "Command was interrupted and termination was requested. Read outputFile for captured output."
                .to_string()
            }
        },
    }
}

fn failure_message(failure: &CommandProcessFailure) -> String {
    match failure {
        CommandProcessFailure::Exited { exit_code } => format!(
            "Command exited with code {}",
            exit_code.map_or_else(|| "unknown".to_string(), |code| code.to_string())
        ),
        CommandProcessFailure::Wait { message } | CommandProcessFailure::Output { message, .. } => {
            message.clone()
        }
    }
}

pub(super) fn truncate_text(text: &str, max_output_chars: usize) -> TruncatedOutput {
    let head = max_output_chars / 2;
    let tail = max_output_chars.saturating_sub(head);
    TruncationStrategy::new(head, tail).truncate(text)
}

impl CommandTerminationReason {
    fn message_fragment(self) -> &'static str {
        match self {
            Self::TimedOut => "timed out",
            Self::Cancelled => "was cancelled",
        }
    }
}

impl From<&CommandOutputSnapshot> for OutputTruncation {
    fn from(snapshot: &CommandOutputSnapshot) -> Self {
        Self {
            stdout: snapshot.stdout.clone(),
            stderr: snapshot.stderr.clone(),
        }
    }
}

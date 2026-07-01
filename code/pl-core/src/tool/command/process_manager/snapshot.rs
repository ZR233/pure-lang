use crate::tool::truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};

use super::{
    CommandOutputSnapshot, CommandProcessPhase, CommandProcessResult, CommandProcessState,
    CommandTerminationReason,
};

pub(super) fn status_for_state(state: &CommandProcessState) -> &'static str {
    match state.phase {
        CommandProcessPhase::Running
        | CommandProcessPhase::Terminating(_)
        | CommandProcessPhase::Draining(_) => "running",
        CommandProcessPhase::Final(CommandProcessResult::Completed) => "completed",
        CommandProcessPhase::Final(CommandProcessResult::Failed) => "failed",
        CommandProcessPhase::Final(CommandProcessResult::TimedOut) => "timedOut",
        CommandProcessPhase::Final(CommandProcessResult::Interrupted) => "interrupted",
    }
}

pub(super) fn message_for_state(
    state: &CommandProcessState,
    process_id: Option<&str>,
    output_file: &std::path::Path,
) -> String {
    if let Some(error) = &state.error {
        return format!(
            "{error}. Full command output is available at '{}'.",
            output_file.display()
        );
    }
    if let Some(reason) = state.termination_reason() {
        return format!(
            "Command {} and termination is in progress. Use write_stdin with processId '{}' and empty chars to wait or poll. Read outputFile for captured output.",
            reason.message_fragment(),
            process_id.unwrap_or_default()
        );
    }
    match status_for_state(state) {
        "running" if state.is_draining_output() => format!(
            "Command exited and is draining remaining output. Use write_stdin with processId '{}' and empty chars to wait or poll. Read outputFile for complete output.",
            process_id.unwrap_or_default()
        ),
        "running" => format!(
            "Command is still running. Use write_stdin with processId '{}' to wait, poll, or send input. Read outputFile for complete output.",
            process_id.unwrap_or_default()
        ),
        "completed" => {
            "Command completed successfully. Read outputFile for complete output if needed."
                .to_string()
        }
        "failed" => format!(
            "Command exited with code {}. Read outputFile for complete stdout/stderr.",
            state
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ),
        "timedOut" => {
            "Command timed out and was terminated. Read outputFile for captured output.".to_string()
        }
        "interrupted" => {
            "Command was interrupted and termination was requested. Read outputFile for captured output."
                .to_string()
        }
        _ => "Command status is unavailable.".to_string(),
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
            Self::Interrupted => "was interrupted",
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

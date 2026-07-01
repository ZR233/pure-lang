use pretty_assertions::assert_eq;

use super::snapshot::{message_for_state, status_for_state};
use super::{
    CommandProcessPhase, CommandProcessState, CommandProcessTransition, HeadTailBuffer,
    INTERNAL_BUFFER_BYTES, StreamKind,
};

fn running_state() -> CommandProcessState {
    CommandProcessState {
        phase: CommandProcessPhase::Running,
        exit_code: None,
        stdout_open: true,
        stderr_open: true,
        stdout: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
        stderr: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
        output_revision: 0,
        error: None,
    }
}

#[test]
fn process_exit_waits_for_output_streams_before_final_status() {
    let mut state = running_state();

    state.apply_transition(CommandProcessTransition::ProcessExited { exit_code: Some(0) });
    assert_eq!(status_for_state(&state), "running");
    assert!(!state.can_accept_input());

    state.apply_transition(CommandProcessTransition::StreamClosed(StreamKind::Stdout));
    assert_eq!(status_for_state(&state), "running");

    state.apply_transition(CommandProcessTransition::StreamClosed(StreamKind::Stderr));
    assert_eq!(status_for_state(&state), "completed");
    assert!(state.is_final());
}

#[test]
fn termination_reason_survives_until_streams_are_drained() {
    let mut state = running_state();

    state.apply_transition(CommandProcessTransition::TimedOut);
    assert_eq!(status_for_state(&state), "running");
    assert!(state.timed_out());

    state.apply_transition(CommandProcessTransition::ProcessExited { exit_code: None });
    assert_eq!(status_for_state(&state), "running");

    state.apply_transition(CommandProcessTransition::StreamClosed(StreamKind::Stdout));
    state.apply_transition(CommandProcessTransition::StreamClosed(StreamKind::Stderr));

    assert_eq!(status_for_state(&state), "timedOut");
    assert!(state.timed_out());
    assert!(state.is_final());
}

#[test]
fn draining_output_message_only_suggests_polling() {
    let mut state = running_state();

    state.apply_transition(CommandProcessTransition::ProcessExited { exit_code: Some(0) });

    let message = message_for_state(&state, Some("proc-1"), std::path::Path::new("output.log"));

    assert!(message.contains("draining remaining output"));
    assert!(message.contains("empty chars"));
    assert!(!message.contains("send input"));
}

#[test]
fn terminating_message_only_suggests_polling() {
    let mut timed_out = running_state();
    timed_out.apply_transition(CommandProcessTransition::TimedOut);
    let timeout_message = message_for_state(
        &timed_out,
        Some("proc-1"),
        std::path::Path::new("output.log"),
    );

    assert!(timeout_message.contains("timed out"));
    assert!(timeout_message.contains("termination is in progress"));
    assert!(timeout_message.contains("empty chars"));
    assert!(!timeout_message.contains("send input"));

    let mut interrupted = running_state();
    interrupted.apply_transition(CommandProcessTransition::Interrupted);
    let interrupted_message = message_for_state(
        &interrupted,
        Some("proc-2"),
        std::path::Path::new("output.log"),
    );

    assert!(interrupted_message.contains("was interrupted"));
    assert!(interrupted_message.contains("termination is in progress"));
    assert!(interrupted_message.contains("empty chars"));
    assert!(!interrupted_message.contains("send input"));
}

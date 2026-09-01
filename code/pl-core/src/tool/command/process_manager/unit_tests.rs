use pretty_assertions::assert_eq;

use super::snapshot::message_for_state;
use super::*;
use crate::tool::LocalCommandBackend;

fn running_state() -> CommandProcessState {
    CommandProcessState::new(true, true)
}

#[test]
fn output_is_accumulated_for_artifacts_but_claimed_once_by_model_snapshots() {
    let mut state = running_state();

    assert_eq!(state.record_output(StreamKind::Stdout, b"first"), 1);
    assert_eq!(state.stdout.display_text(), "first");
    assert_eq!(state.pending_stdout.take_display_text(), "first");
    assert_eq!(state.pending_stdout.take_display_text(), "");

    assert_eq!(state.record_output(StreamKind::Stdout, b" second"), 2);
    assert_eq!(state.stdout.display_text(), "first second");
    assert_eq!(state.pending_stdout.take_display_text(), " second");
}

#[test]
fn process_exit_waits_for_output_streams_before_final_status() {
    let mut state = running_state();

    state.apply_transition(CommandProcessTransition::ProcessExited { exit_code: Some(0) });
    assert!(matches!(
        state.lifecycle,
        CommandProcessLifecycle::Draining(_)
    ));
    assert!(!state.can_accept_input());

    state.apply_transition(CommandProcessTransition::StreamClosed(StreamKind::Stdout));
    assert!(matches!(
        state.lifecycle,
        CommandProcessLifecycle::Draining(_)
    ));

    state.apply_transition(CommandProcessTransition::StreamClosed(StreamKind::Stderr));
    assert!(matches!(
        state.lifecycle.final_result(),
        Some(CommandProcessFinalResult::Succeeded { exit_code: 0 })
    ));
    assert!(state.is_final());
}

#[test]
fn termination_reason_survives_until_streams_are_drained() {
    let mut state = running_state();

    state.apply_transition(CommandProcessTransition::TimeOut);
    assert!(matches!(
        state.lifecycle,
        CommandProcessLifecycle::Terminating(_)
    ));
    assert!(state.lifecycle.is_timed_out());

    state.apply_transition(CommandProcessTransition::ProcessExited { exit_code: None });
    assert!(matches!(
        state.lifecycle,
        CommandProcessLifecycle::Draining(_)
    ));

    state.apply_transition(CommandProcessTransition::StreamClosed(StreamKind::Stdout));
    state.apply_transition(CommandProcessTransition::StreamClosed(StreamKind::Stderr));

    assert!(matches!(
        state.lifecycle.final_result(),
        Some(CommandProcessFinalResult::TimedOut)
    ));
    assert!(state.lifecycle.is_timed_out());
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
    timed_out.apply_transition(CommandProcessTransition::TimeOut);
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
    interrupted.apply_transition(CommandProcessTransition::Cancel);
    let interrupted_message = message_for_state(
        &interrupted,
        Some("proc-2"),
        std::path::Path::new("output.log"),
    );

    assert!(interrupted_message.contains("was cancelled"));
    assert!(interrupted_message.contains("termination is in progress"));
    assert!(interrupted_message.contains("empty chars"));
    assert!(!interrupted_message.contains("send input"));
}

#[tokio::test]
async fn process_ids_are_unique_across_manager_instances_and_rebuilds() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalCommandBackend::new(root.path()));
    let first = CommandProcessManager::new(backend.clone());
    let second = CommandProcessManager::new(backend.clone());

    let (first_id, second_id) =
        tokio::join!(first.reserve_process_id(), second.reserve_process_id());
    let first_id = first_id.unwrap();
    let second_id = second_id.unwrap();
    assert_ne!(first_id, second_id);

    let rebuilt = CommandProcessManager::new(backend);
    let rebuilt_id = rebuilt.reserve_process_id().await.unwrap();
    assert_ne!(rebuilt_id, first_id);
    assert_ne!(rebuilt_id, second_id);

    first.release_start_reservation().await;
    second.release_start_reservation().await;
    rebuilt.release_start_reservation().await;
}

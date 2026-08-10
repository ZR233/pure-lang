//! 生产合并验证器的结构化失败证据回归。

use super::super::MergeVerificationFailureKind;
use super::verifier::{MergeVerificationCommand, ProductionMergeVerifier};

#[tokio::test]
async fn nonzero_process_exit_returns_a_structured_failed_step() {
    let selected = fake_failing_command();
    let expected_command = selected.command.clone();

    let steps = ProductionMergeVerifier::verify_commands(vec![selected]).await;

    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].cwd, ".");
    assert_eq!(steps[0].command, expected_command);
    assert!(!steps[0].success);
    assert_eq!(steps[0].exit_code, Some(19));
    assert_eq!(
        steps[0].failure_kind,
        Some(MergeVerificationFailureKind::NonZeroExit)
    );
    assert!(steps[0].output.contains("fake verifier failure"));
}

#[tokio::test]
async fn process_start_failure_returns_a_structured_failed_step() {
    let steps = ProductionMergeVerifier::verify_commands(vec![MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        relative_working_directory: "code".to_string(),
        command: vec!["pure-studio-command-that-does-not-exist".to_string()],
    }])
    .await;

    assert_eq!(steps[0].cwd, "code");
    assert_eq!(steps[0].exit_code, None);
    assert_eq!(
        steps[0].failure_kind,
        Some(MergeVerificationFailureKind::StartFailed)
    );
    assert!(steps[0].output.contains("failed to start"));
}

#[tokio::test]
async fn process_timeout_returns_a_structured_failed_step() {
    let steps = ProductionMergeVerifier::verify_commands(vec![fake_slow_command()]).await;

    assert_eq!(steps[0].cwd, ".");
    assert_eq!(steps[0].exit_code, None);
    assert_eq!(
        steps[0].failure_kind,
        Some(MergeVerificationFailureKind::TimedOut)
    );
    assert!(steps[0].output.contains("timed out"));
}

#[cfg(windows)]
fn fake_failing_command() -> MergeVerificationCommand {
    MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        relative_working_directory: ".".to_string(),
        command: vec![
            "cmd.exe".to_string(),
            "/D".to_string(),
            "/C".to_string(),
            "echo fake verifier failure 1>&2 & exit 19".to_string(),
        ],
    }
}

#[cfg(windows)]
fn fake_slow_command() -> MergeVerificationCommand {
    MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        relative_working_directory: ".".to_string(),
        command: vec![
            "cmd.exe".to_string(),
            "/D".to_string(),
            "/C".to_string(),
            "ping -n 4 127.0.0.1 >NUL".to_string(),
        ],
    }
}

#[cfg(not(windows))]
fn fake_failing_command() -> MergeVerificationCommand {
    MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        relative_working_directory: ".".to_string(),
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'fake verifier failure\\n' >&2; exit 19".to_string(),
        ],
    }
}

#[cfg(not(windows))]
fn fake_slow_command() -> MergeVerificationCommand {
    MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        relative_working_directory: ".".to_string(),
        command: vec!["sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
    }
}

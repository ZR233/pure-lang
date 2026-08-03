//! 生产合并验证器的结构化失败证据回归。

use super::verifier::{MergeVerificationCommand, ProductionMergeVerifier};

#[tokio::test]
async fn nonzero_process_exit_returns_a_structured_failed_step() {
    let selected = fake_failing_command();
    let expected_command = selected.command.clone();

    let steps = ProductionMergeVerifier::verify_commands(vec![selected]).await;

    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].command, expected_command);
    assert!(!steps[0].success);
    assert!(steps[0].output.contains("fake verifier failure"));
}

#[cfg(windows)]
fn fake_failing_command() -> MergeVerificationCommand {
    MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        command: vec![
            "cmd.exe".to_string(),
            "/D".to_string(),
            "/C".to_string(),
            "echo fake verifier failure 1>&2 & exit /B 19".to_string(),
        ],
    }
}

#[cfg(not(windows))]
fn fake_failing_command() -> MergeVerificationCommand {
    MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'fake verifier failure\\n' >&2; exit 19".to_string(),
        ],
    }
}

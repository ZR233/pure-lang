//! 生产合并验证器的结构化失败证据回归。

use std::path::{Path, PathBuf};

use super::verifier::{
    MergeVerificationCommand, ProductionMergeVerifier, select_merge_verification_commands,
};

#[test]
fn selector_keeps_production_commands_and_working_directories_auditable() {
    let workspace = Path::new("C:/repo");

    let selected = select_merge_verification_commands(
        workspace,
        &[
            "code/pl-core/src/lib.rs".to_string(),
            "code/pure-studio/windows/runner/main.cpp".to_string(),
        ],
    );

    assert_eq!(
        selected,
        vec![
            MergeVerificationCommand {
                working_directory: PathBuf::from("C:/repo"),
                command: vec![
                    "cargo".to_string(),
                    "fmt".to_string(),
                    "--all".to_string(),
                    "--check".to_string(),
                ],
            },
            MergeVerificationCommand {
                working_directory: PathBuf::from("C:/repo/code/pure-studio"),
                command: vec![
                    "flutter".to_string(),
                    "--no-version-check".to_string(),
                    "analyze".to_string(),
                ],
            },
        ]
    );
}

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

#[tokio::test]
async fn silent_nonzero_process_exit_still_returns_diagnostic_output() {
    let steps = ProductionMergeVerifier::verify_commands(vec![fake_silent_failing_command()]).await;

    assert_eq!(steps.len(), 1);
    assert!(!steps[0].success);
    assert_eq!(
        steps[0].output,
        "command exited unsuccessfully without output"
    );
}

#[tokio::test]
async fn process_infrastructure_error_returns_a_structured_failed_step() {
    let command = vec!["pure-verifier-command-that-does-not-exist".to_string()];

    let steps = ProductionMergeVerifier::verify_commands(vec![MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        command: command.clone(),
    }])
    .await;

    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].command, command);
    assert!(!steps[0].success);
    assert!(!steps[0].output.is_empty());
    assert!(steps[0].output.contains("failed to start"));
}

#[tokio::test]
async fn empty_command_configuration_returns_a_structured_failed_step() {
    let steps = ProductionMergeVerifier::verify_commands(vec![MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        command: Vec::new(),
    }])
    .await;

    assert_eq!(steps.len(), 1);
    assert!(steps[0].command.is_empty());
    assert!(!steps[0].success);
    assert_eq!(steps[0].output, "merge verifier command is empty");
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

#[cfg(windows)]
fn fake_silent_failing_command() -> MergeVerificationCommand {
    MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        command: vec![
            "cmd.exe".to_string(),
            "/D".to_string(),
            "/C".to_string(),
            "exit /B 23".to_string(),
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

#[cfg(not(windows))]
fn fake_silent_failing_command() -> MergeVerificationCommand {
    MergeVerificationCommand {
        working_directory: std::env::temp_dir(),
        command: vec!["sh".to_string(), "-c".to_string(), "exit 23".to_string()],
    }
}

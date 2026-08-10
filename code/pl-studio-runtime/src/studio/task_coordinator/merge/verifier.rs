use std::path::Path;

use crate::studio::task_coordinator::MergeVerificationStep;

const MAX_VERIFICATION_OUTPUT_BYTES: usize = 32 * 1024;

pub(crate) struct ProductionMergeVerifier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeVerificationCommand {
    pub(crate) working_directory: std::path::PathBuf,
    pub(crate) relative_working_directory: String,
    pub(crate) command: Vec<String>,
}

impl ProductionMergeVerifier {
    pub(crate) async fn verify_commands(
        commands: Vec<MergeVerificationCommand>,
    ) -> Vec<MergeVerificationStep> {
        let mut steps = Vec::new();
        for command in commands {
            let step = run_check(command).await;
            let success = step.success;
            steps.push(step);
            if !success {
                break;
            }
        }
        steps
    }
}

pub(crate) fn select_merge_verification_commands(
    workspace: &Path,
    changed_files: &[String],
) -> Vec<MergeVerificationCommand> {
    let mut commands = Vec::new();
    if changed_files.iter().any(|path| path.ends_with(".rs")) {
        commands.push(MergeVerificationCommand {
            working_directory: workspace.to_path_buf(),
            relative_working_directory: ".".to_string(),
            command: vec![
                "cargo".to_string(),
                "fmt".to_string(),
                "--all".to_string(),
                "--check".to_string(),
            ],
        });
    }
    if changed_files
        .iter()
        .any(|path| path.starts_with("code/pure-studio/"))
    {
        commands.push(MergeVerificationCommand {
            working_directory: workspace.join("code/pure-studio"),
            relative_working_directory: "code/pure-studio".to_string(),
            command: vec![
                "flutter".to_string(),
                "--no-version-check".to_string(),
                "analyze".to_string(),
            ],
        });
    }
    commands
}

async fn run_check(selected: MergeVerificationCommand) -> MergeVerificationStep {
    let cwd = selected.relative_working_directory;
    let command = selected.command;
    let Some((program, arguments)) = command.split_first() else {
        return MergeVerificationStep {
            cwd,
            command,
            success: false,
            exit_code: None,
            failure_kind: Some(super::super::MergeVerificationFailureKind::RuntimeFailed),
            output: "merge verifier command is empty".to_string(),
            output_truncated: false,
        };
    };
    match super::process::run_process(&selected.working_directory, program, arguments.to_vec())
        .await
    {
        Ok(output) => {
            let detail = if !output.success && output.combined.is_empty() {
                "command exited unsuccessfully without output".to_string()
            } else {
                output.combined
            };
            let (detail, output_truncated) = bounded_output(detail);
            MergeVerificationStep {
                cwd,
                command,
                success: output.success,
                exit_code: output.exit_code,
                failure_kind: (!output.success)
                    .then_some(super::super::MergeVerificationFailureKind::NonZeroExit),
                output: detail,
                output_truncated,
            }
        }
        Err(error) => {
            let (output, output_truncated) = bounded_output(error.message);
            MergeVerificationStep {
                cwd,
                command,
                success: false,
                exit_code: None,
                failure_kind: Some(error.kind),
                output,
                output_truncated,
            }
        }
    }
}

fn bounded_output(output: String) -> (String, bool) {
    if output.len() <= MAX_VERIFICATION_OUTPUT_BYTES {
        return (output, false);
    }
    let mut boundary = output.len() - MAX_VERIFICATION_OUTPUT_BYTES;
    while !output.is_char_boundary(boundary) {
        boundary += 1;
    }
    (output[boundary..].to_string(), true)
}

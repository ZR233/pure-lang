use std::path::Path;

use crate::studio::task_coordinator::MergeVerificationStep;

pub(crate) struct ProductionMergeVerifier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeVerificationCommand {
    pub(crate) working_directory: std::path::PathBuf,
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
    let command = selected.command;
    let Some((program, arguments)) = command.split_first() else {
        return MergeVerificationStep {
            command,
            success: false,
            output: "merge verifier command is empty".to_string(),
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
            MergeVerificationStep {
                command,
                success: output.success,
                output: detail,
            }
        }
        Err(error) => MergeVerificationStep {
            command,
            success: false,
            output: format!("{error:#}"),
        },
    }
}

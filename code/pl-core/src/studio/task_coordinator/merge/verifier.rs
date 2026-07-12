use std::path::Path;

use anyhow::{Result, bail};

use super::git::run_git;
use crate::studio::task_coordinator::{MergeVerificationRequest, MergeVerificationStep};

/// 验证 coordinator 已应用但尚未提交的 merge；实现者不得修改 workspace。
pub(crate) trait MergeVerifier: Send + Sync {
    fn verify(
        &self,
        request: MergeVerificationRequest,
    ) -> impl std::future::Future<Output = Result<Vec<MergeVerificationStep>>> + Send;
}

pub(super) struct ProductionMergeVerifier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeVerificationCommand {
    pub(crate) working_directory: std::path::PathBuf,
    pub(crate) command: Vec<String>,
}

impl ProductionMergeVerifier {
    pub(super) async fn verify_commands(
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

impl MergeVerifier for ProductionMergeVerifier {
    async fn verify(
        &self,
        request: MergeVerificationRequest,
    ) -> Result<Vec<MergeVerificationStep>> {
        let commands = select_merge_verification_commands(
            Path::new(&request.workspace_root),
            &request.changed_files,
        );
        Ok(Self::verify_commands(commands).await)
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
        .any(|path| path.starts_with("code/pure-studio-flutter/"))
    {
        commands.push(MergeVerificationCommand {
            working_directory: workspace.join("code/pure-studio-flutter"),
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

pub(super) async fn abort_merge(workspace: &Path) -> Result<()> {
    let output = run_git(workspace, vec!["merge".into(), "--abort".into()]).await?;
    if output.success {
        Ok(())
    } else {
        bail!("git merge --abort failed: {}", output.stderr_lossy())
    }
}

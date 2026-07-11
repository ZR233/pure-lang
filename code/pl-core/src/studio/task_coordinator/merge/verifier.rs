use std::path::Path;

use anyhow::{Context, Result, bail};

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

impl MergeVerifier for ProductionMergeVerifier {
    async fn verify(
        &self,
        request: MergeVerificationRequest,
    ) -> Result<Vec<MergeVerificationStep>> {
        let commands = select_merge_verification_commands(
            Path::new(&request.workspace_root),
            &request.changed_files,
        );
        let mut steps = Vec::new();
        for command in commands {
            let step = run_check(command).await?;
            if !step.success {
                bail!("merge verification failed: {}", step.output);
            }
            steps.push(step);
        }
        Ok(steps)
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

async fn run_check(selected: MergeVerificationCommand) -> Result<MergeVerificationStep> {
    let (program, arguments) = selected
        .command
        .split_first()
        .context("merge verifier command is empty")?;
    let output =
        super::process::run_process(&selected.working_directory, program, arguments.to_vec())
            .await?;
    Ok(MergeVerificationStep {
        command: selected.command,
        success: output.success,
        output: output.combined,
    })
}

pub(super) async fn abort_merge(workspace: &Path) -> Result<()> {
    let output = run_git(workspace, vec!["merge".into(), "--abort".into()]).await?;
    if output.success {
        Ok(())
    } else {
        bail!("git merge --abort failed: {}", output.stderr_lossy())
    }
}

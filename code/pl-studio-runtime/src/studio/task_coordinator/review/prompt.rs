use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::super::{TaskCoordinator, TaskRunRecord};

pub(crate) async fn build_review_prompt(
    coordinator: &TaskCoordinator,
    run: &TaskRunRecord,
) -> Result<String> {
    let diff = task_diff(run).await?;
    let outcomes = coordinator.store.list_agent_outcomes(&run.id).await?;
    let prior_reviews = coordinator.store.list_review_rounds(&run.id).await?;
    let design_index = design_index(Path::new(&run.workspace_root))?;
    let outcome_json = serde_json::to_string_pretty(&outcomes)?;
    let prior_json = serde_json::to_string_pretty(&prior_reviews)?;
    Ok(format!(
        "# 审查任务\n\n## 用户确认的完整 plan\n{}\n\n## 综合 diff（{}..{}）\n```diff\n{}\n```\n\n## 代理结果与验证摘要\n```json\n{}\n```\n\n## 既往审查\n```json\n{}\n```\n\n## design 文件索引\n{}\n",
        run.plan,
        run.base_commit,
        run.expected_head,
        diff,
        outcome_json,
        prior_json,
        design_index
            .iter()
            .map(|path| format!("- {path}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

async fn task_diff(run: &TaskRunRecord) -> Result<String> {
    let mut command = tokio::process::Command::new("git");
    command
        .arg("-C")
        .arg(&run.workspace_root)
        .args([
            "diff",
            "--find-renames",
            "--find-copies",
            &format!("{}..{}", run.base_commit, run.expected_head),
            "--",
            ".",
            ":(exclude)design/**",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(120), command.output())
        .await
        .context("review diff command timed out")??;
    if !output.status.success() {
        bail!(
            "failed to build review diff: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("review diff is not UTF-8")
}

fn design_index(workspace: &Path) -> Result<Vec<String>> {
    let design = workspace.join("design");
    if !design.is_dir() {
        return Ok(Vec::new());
    }
    let mut pending = vec![design];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .with_context(|| format!("failed to read `{}`", directory.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
                files.push(relative_path(workspace, path)?);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn relative_path(workspace: &Path, path: PathBuf) -> Result<String> {
    Ok(path
        .strip_prefix(workspace)
        .context("design index path escaped workspace")?
        .to_string_lossy()
        .replace('\\', "/"))
}

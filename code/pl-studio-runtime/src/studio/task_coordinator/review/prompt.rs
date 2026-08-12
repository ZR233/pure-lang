use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use pl_core::path_safety::{metadata_if_real, real_directory_entries};
use serde::Serialize;

use super::super::{
    ReviewRoundRecord, ReviewScope, TaskCoordinator, WorkUnitRecord, WorkUnitStatus,
};
use super::{ModelCompletion, ModelWorkUnit};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewFocus {
    work_unit_id: String,
    title: String,
    status: WorkUnitStatus,
    scope_hints: Vec<String>,
}

pub(crate) async fn build_review_prompt(
    coordinator: &TaskCoordinator,
    round: &ReviewRoundRecord,
) -> Result<String> {
    let run = coordinator
        .store
        .read_task_run(&round.task_run_id)
        .await?
        .context("review task run not found")?;
    let prior_reviews = coordinator.store.list_review_rounds(&run.id).await?;
    let design_index = design_index(Path::new(&run.workspace_root))?;
    let target = match round.scope {
        ReviewScope::Delivery => {
            let completion_id = round
                .completion_id
                .as_deref()
                .context("delivery review has no completion id")?;
            let completion = coordinator
                .store
                .list_work_completions(&run.id)
                .await?
                .into_iter()
                .find(|completion| completion.id == completion_id)
                .context("delivery review completion not found")?;
            let work_units = coordinator.store.list_work_units(&run.id).await?;
            let target_work_unit = work_units
                .iter()
                .find(|work_unit| work_unit.id == completion.work_unit_id)
                .context("delivery review work unit not found")?;
            let target_focus = ReviewFocus::from(target_work_unit);
            let sibling_focus = work_units
                .iter()
                .filter(|work_unit| work_unit.id != completion.work_unit_id)
                .map(ReviewFocus::from)
                .collect::<Vec<_>>();
            let diff = match completion.head_commit.as_deref() {
                Some(head) => {
                    git_diff(
                        &completion.worktree_path,
                        &completion.base_commit,
                        head,
                        false,
                    )
                    .await?
                }
                None => String::new(),
            };
            format!(
                "## Scope\nDelivery\n\n## Delivery review boundary\nReview the complete exact Completion diff. scopeHints are planning and review-focus hints only; files outside them remain in scope. Sibling WorkUnits are deferred integration context only: do not report their unmerged or missing files, cross-WorkUnit integration, or task-wide completeness as delivery findings. Those concerns belong to the integrated review after merge.\n\n## Target WorkUnit focus\n```json\n{}\n```\n\n## Sibling WorkUnit focus (deferred integration context only)\n```json\n{}\n```\n\n## Completion\n```json\n{}\n```\n\n## Exact completion diff\n```diff\n{}\n```",
                serde_json::to_string_pretty(&target_focus)?,
                serde_json::to_string_pretty(&sibling_focus)?,
                serde_json::to_string_pretty(&ModelCompletion::new(&run, &completion))?,
                diff
            )
        }
        ReviewScope::Integrated => {
            let diff = git_diff(
                &run.workspace_root,
                &run.base_commit,
                &run.expected_head,
                true,
            )
            .await?;
            let completions = coordinator
                .store
                .list_work_completions(&run.id)
                .await?
                .iter()
                .map(|completion| ModelCompletion::new(&run, completion))
                .collect::<Vec<_>>();
            let work_units = coordinator
                .store
                .list_work_units(&run.id)
                .await?
                .iter()
                .map(|work_unit| ModelWorkUnit::new(&run, work_unit))
                .collect::<Vec<_>>();
            format!(
                "## Scope\nIntegrated\n\n## Task HEAD\n{}\n\n## Integrated diff\n```diff\n{}\n```\n\n## Work completions\n```json\n{}\n```\n\n## WorkUnit execution state\n```json\n{}\n```",
                run.expected_head,
                diff,
                serde_json::to_string_pretty(&completions)?,
                serde_json::to_string_pretty(&work_units)?
            )
        }
    };
    Ok(format!(
        "# 审查任务\n\n## 用户确认的完整 plan\n{}\n\n{}\n\n## finding 输出契约\n每条 finding 必须给出可执行的 `recommendation`：写清「改成什么、为什么」，必要时给内联代码片段或精确到函数/行号的最小改法，让 executor 能据此直接 rework。只描述问题而不给出改法的 finding 会被拒绝。\n示例：`recommendation`: \"将 `Config::load` 中的 `unwrap()` 改为传播错误：`let bytes = std::fs::read(&path)?;`，因为配置缺失时应让上层决定回退而非 panic。\"\n\n## 既往审查\n```json\n{}\n```\n\n## design 文件索引\n{}\n",
        run.plan,
        target,
        serde_json::to_string_pretty(&prior_reviews)?,
        design_index
            .iter()
            .map(|path| format!("- {path}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

impl From<&WorkUnitRecord> for ReviewFocus {
    fn from(work_unit: &WorkUnitRecord) -> Self {
        Self {
            work_unit_id: work_unit.id.clone(),
            title: work_unit.title.clone(),
            status: work_unit.status,
            scope_hints: work_unit.scope_hints.clone(),
        }
    }
}

async fn git_diff(workspace: &str, base: &str, head: &str, exclude_design: bool) -> Result<String> {
    let mut command = tokio::process::Command::new("git");
    command.arg("-C").arg(workspace).args([
        "diff",
        "--find-renames",
        "--find-copies",
        &format!("{base}..{head}"),
        "--",
        ".",
    ]);
    if exclude_design {
        command.arg(":(exclude)design/**");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process::configure_background_command(&mut command);
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
    if !metadata_if_real(&design)
        .map_err(anyhow::Error::from)?
        .is_some_and(|metadata| metadata.is_dir())
    {
        return Ok(Vec::new());
    }
    let mut pending = vec![design];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for path in real_directory_entries(&directory)
            .with_context(|| format!("failed to read `{}`", directory.display()))?
        {
            let Some(metadata) = metadata_if_real(&path).map_err(anyhow::Error::from)? else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("md")
            {
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

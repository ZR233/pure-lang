use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use pl_core::path_safety::{metadata_if_real, real_directory_entries};
use serde::Serialize;

use super::super::spawn::TaskExecutorBlueprint;
use super::super::{ReviewRoundRecord, ReviewScope, TaskCoordinator, WorkUnit, WorkUnitStateKind};
use super::{ModelCompletion, ModelWorkUnit};

const COMMON_TEMPLATE: &str = include_str!("../../../prompts/review/common.md");
const DELIVERY_TEMPLATE: &str = include_str!("../../../prompts/review/delivery.md");
const INTEGRATED_TEMPLATE: &str = include_str!("../../../prompts/review/integrated.md");

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewFocus {
    work_unit_id: String,
    title: String,
    status: WorkUnitStateKind,
    scope_hints: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewBlueprint<'a> {
    blueprint_fingerprint: &'a str,
    blueprint: &'a TaskExecutorBlueprint,
}

pub(crate) async fn build_review_prompt(
    coordinator: &TaskCoordinator,
    round: &ReviewRoundRecord,
) -> Result<String> {
    let aggregate = coordinator
        .task_runtime
        .aggregate_for_run(&round.task_run_id)
        .await
        .context("review task run is not resident")?;
    let run = aggregate.facts.run;
    let prior_reviews = aggregate.facts.reviews;
    let work_units = aggregate.facts.work_units;
    let completions = aggregate.facts.completions;
    let design_index = design_index(Path::new(&run.workspace_root))?;
    let coverage = round
        .file_reviews
        .as_ref()
        .context("review round has no frozen changed-files snapshot")?;
    let changed_files = coverage.expected_paths();
    let scope_block = match round.scope {
        ReviewScope::Delivery => {
            let completion_id = round
                .completion_id
                .as_deref()
                .context("delivery review has no completion id")?;
            let completion = completions
                .iter()
                .find(|completion| completion.id == completion_id)
                .context("delivery review completion not found")?;
            let target_work_unit = work_units
                .iter()
                .find(|work_unit| work_unit.id == completion.work_unit_id)
                .context("delivery review work unit not found")?;
            let (_, handoff) = coordinator
                .store
                .read_work_unit_handoff(&target_work_unit.id)
                .await?
                .context("delivery review work unit has no durable handoff")?;
            let target_focus = ReviewFocus::from(target_work_unit);
            let sibling_focus = work_units
                .iter()
                .filter(|work_unit| work_unit.id != completion.work_unit_id)
                .map(ReviewFocus::from)
                .collect::<Vec<_>>();
            let diff = "TaskService 不读取 Git diff；请以冻结的 changedFiles 与 Completion 声明为审计范围，并直接读取对应文件。".to_string();
            render_template(
                DELIVERY_TEMPLATE,
                [
                    (
                        "TARGET_FOCUS_JSON",
                        serde_json::to_string_pretty(&target_focus)?,
                    ),
                    (
                        "SIBLING_FOCUS_JSON",
                        serde_json::to_string_pretty(&sibling_focus)?,
                    ),
                    (
                        "COMPLETION_JSON",
                        serde_json::to_string_pretty(&ModelCompletion::new(&run, completion))?,
                    ),
                    (
                        "HANDOFF_JSON",
                        serde_json::to_string_pretty(&ReviewBlueprint {
                            blueprint_fingerprint: &handoff.blueprint_fingerprint,
                            blueprint: &handoff.blueprint,
                        })?,
                    ),
                    (
                        "CHANGED_FILES_JSON",
                        serde_json::to_string_pretty(&changed_files)?,
                    ),
                    ("DIFF", diff),
                ],
            )?
        }
        ReviewScope::Integrated => {
            let diff = "TaskService 不读取 Git diff；请以持久化 Completion/Merge 声明和冻结的 changedFiles 为审计范围。".to_string();
            let completions = completions
                .iter()
                .map(|completion| ModelCompletion::new(&run, completion))
                .collect::<Vec<_>>();
            let work_units = work_units
                .iter()
                .map(|work_unit| ModelWorkUnit::new(&run, work_unit, None))
                .collect::<Vec<_>>();
            render_template(
                INTEGRATED_TEMPLATE,
                [
                    ("TASK_HEAD", round.reviewed_head.clone()),
                    (
                        "CHANGED_FILES_JSON",
                        serde_json::to_string_pretty(&changed_files)?,
                    ),
                    ("DIFF", diff),
                    (
                        "COMPLETIONS_JSON",
                        serde_json::to_string_pretty(&completions)?,
                    ),
                    (
                        "WORK_UNITS_JSON",
                        serde_json::to_string_pretty(&work_units)?,
                    ),
                ],
            )?
        }
    };
    render_template(
        COMMON_TEMPLATE,
        [
            (
                "PLAN",
                run.plan_content()
                    .context("TaskRun has no frozen plan")?
                    .to_string(),
            ),
            ("SCOPE_BLOCK", scope_block),
            (
                "PRIOR_REVIEWS_JSON",
                serde_json::to_string_pretty(&prior_reviews)?,
            ),
            (
                "DESIGN_INDEX",
                if design_index.is_empty() {
                    "- （无 `design/**` Markdown 文档；跳过 design 读取与引用门禁）".to_string()
                } else {
                    design_index
                        .iter()
                        .map(|path| format!("- {path}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                },
            ),
        ],
    )
}

fn render_template<const N: usize>(
    template: &str,
    replacements: [(&str, String); N],
) -> Result<String> {
    let mut values = BTreeMap::new();
    for (name, value) in replacements {
        if values.insert(format!("{{{{{name}}}}}"), value).is_some() {
            bail!("duplicate review prompt replacement `{name}`");
        }
    }
    let mut seen = BTreeSet::new();
    let mut rendered = Vec::new();
    for line in template.lines() {
        if line.contains("{{") || line.contains("}}") {
            let value = values
                .get(line)
                .with_context(|| format!("unknown or non-exclusive prompt placeholder `{line}`"))?;
            if !seen.insert(line.to_string()) {
                bail!("duplicate review prompt placeholder `{line}`");
            }
            rendered.push(value.clone());
        } else {
            rendered.push(line.to_string());
        }
    }
    let missing = values
        .keys()
        .filter(|placeholder| !seen.contains(*placeholder))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "review prompt template is missing placeholders: {}",
            missing.join(", ")
        );
    }
    Ok(rendered.join("\n"))
}

impl From<&WorkUnit> for ReviewFocus {
    fn from(work_unit: &WorkUnit) -> Self {
        Self {
            work_unit_id: work_unit.id.clone(),
            title: work_unit.title.clone(),
            status: work_unit.kind(),
            scope_hints: work_unit.scope_hints.clone(),
        }
    }
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

pub(super) fn has_design_docs(workspace: &Path) -> Result<bool> {
    Ok(!design_index(workspace)?.is_empty())
}

fn relative_path(workspace: &Path, path: PathBuf) -> Result<String> {
    Ok(path
        .strip_prefix(workspace)
        .context("design index path escaped workspace")?
        .to_string_lossy()
        .replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_requires_each_placeholder_on_one_exclusive_line() {
        let rendered =
            render_template("before\n{{VALUE}}\nafter", [("VALUE", "内容".to_string())]).unwrap();
        assert_eq!(rendered, "before\n内容\nafter");

        assert!(render_template("before {{VALUE}}", [("VALUE", "内容".to_string())]).is_err());
        assert!(render_template("before", [("VALUE", "内容".to_string())]).is_err());
        assert!(render_template("{{VALUE}}\n{{VALUE}}", [("VALUE", "内容".to_string())]).is_err());
        assert!(render_template("{{UNKNOWN}}", [("VALUE", "内容".to_string())]).is_err());
    }

    #[test]
    fn markdown_templates_require_complete_chinese_review_and_all_placeholders() {
        for required in [
            "冻结 changed-files 清单中的每个文件都必须审查",
            "调用点、测试、错误路径、边界输入以及跨文件交互",
            "发现第一个问题后必须继续检查",
            "最终一次提交所有合格 finding",
            "排除推测、既有问题、刻意的需求变化和不影响正确性的纯风格 nit",
            "每个 finding 必须给出",
            "read_review_file_coverage",
            "同一 Turn 重试",
        ] {
            assert!(COMMON_TEMPLATE.contains(required), "missing `{required}`");
        }
        assert!(DELIVERY_TEMPLATE.contains("精确 Completion diff"));
        assert!(INTEGRATED_TEMPLATE.contains("跨 WorkUnit 交互"));
        assert!(INTEGRATED_TEMPLATE.contains("`design/**` 已由独立设计门禁负责"));

        let delivery = render_template(
            DELIVERY_TEMPLATE,
            [
                ("TARGET_FOCUS_JSON", "target".to_string()),
                ("SIBLING_FOCUS_JSON", "siblings".to_string()),
                ("COMPLETION_JSON", "completion".to_string()),
                ("HANDOFF_JSON", "handoff".to_string()),
                ("CHANGED_FILES_JSON", "files".to_string()),
                ("DIFF", "diff".to_string()),
            ],
        )
        .unwrap();
        let rendered = render_template(
            COMMON_TEMPLATE,
            [
                ("PLAN", "plan".to_string()),
                ("SCOPE_BLOCK", delivery),
                ("PRIOR_REVIEWS_JSON", "reviews".to_string()),
                ("DESIGN_INDEX", "design/index.md".to_string()),
            ],
        )
        .unwrap();
        assert!(rendered.contains("plan"));
        assert!(rendered.contains("files"));
        assert!(rendered.contains("diff"));
        assert!(!rendered.contains("{{"));
        assert!(!rendered.contains("}}"));
    }

    #[test]
    fn design_requirement_tracks_markdown_index() {
        let workspace = tempfile::tempdir().unwrap();
        assert!(!has_design_docs(workspace.path()).unwrap());

        std::fs::create_dir_all(workspace.path().join("design/nested")).unwrap();
        std::fs::write(workspace.path().join("design/nested/guide.md"), "# Guide\n").unwrap();

        assert!(has_design_docs(workspace.path()).unwrap());
    }
}

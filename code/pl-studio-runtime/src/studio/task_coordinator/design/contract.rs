use std::collections::BTreeSet;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};

use super::super::git::changed_files_between;
use super::super::{TaskRunPhase, TaskRunRecord};

pub(super) fn ensure_plan_declares_design_targets(plan: &str) -> Result<()> {
    if !planned_design_paths(plan).is_empty() {
        return Ok(());
    }
    bail!(
        "confirmed Task plan must list at least one initial design target as an inline-code workspace-relative `design/**/*.md` path"
    )
}

pub(super) fn ensure_initial_patch_covers_plan(
    run: &TaskRunRecord,
    patch_paths: &[String],
) -> Result<()> {
    if run.phase != TaskRunPhase::DesignUpdating {
        return Ok(());
    }
    let required = planned_design_paths(&run.plan);
    let actual = patch_paths
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let missing = required
        .iter()
        .filter(|path| !actual.contains(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "initial task_update_design patch must cover every design target declared in the confirmed plan; missing paths: {}; no files were modified",
        missing.join(", ")
    )
}

pub(super) async fn ensure_committed_design_covers_plan(run: &TaskRunRecord) -> Result<()> {
    let required = planned_design_paths(&run.plan);
    if required.is_empty() {
        return Ok(());
    }
    let design_commit = run
        .design_commit
        .as_deref()
        .context("task_spawn_executor requires a durable design commit")?;
    let changed = changed_files_between(
        Path::new(&run.workspace_root),
        &run.base_commit,
        design_commit,
    )
    .await
    .context("failed to verify confirmed-plan design coverage")?;
    let changed = changed.into_iter().collect::<BTreeSet<_>>();
    let missing = required
        .iter()
        .filter(|path| !changed.contains(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "task_spawn_executor requires the durable design commit to cover every design target declared in the confirmed plan; missing paths: {}",
        missing.join(", ")
    )
}

fn planned_design_paths(plan: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let mut fenced = false;
    for line in plan.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        extract_inline_design_paths(line, &mut paths);
    }
    paths
}

fn extract_inline_design_paths(line: &str, paths: &mut BTreeSet<String>) {
    let mut remainder = line;
    while let Some(opening) = remainder.find('`') {
        remainder = &remainder[opening + 1..];
        let Some(closing) = remainder.find('`') else {
            return;
        };
        let candidate = remainder[..closing].trim();
        if let Some(path) = normalized_markdown_path(candidate) {
            paths.insert(path);
        }
        remainder = &remainder[closing + 1..];
    }
}

fn normalized_markdown_path(raw: &str) -> Option<String> {
    let path = Path::new(raw);
    let components = path.components().collect::<Vec<_>>();
    if components.len() < 2
        || !matches!(components.first(), Some(Component::Normal(part)) if *part == "design")
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.extension().and_then(|extension| extension.to_str()) != Some("md")
        || path.file_name().and_then(|name| name.to_str()) == Some(".md")
        || raw.contains('\\')
        || raw
            .chars()
            .any(|character| matches!(character, '*' | '?' | '[' | ']' | '{' | '}'))
    {
        return None;
    }
    let normalized = components
        .iter()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    (normalized == raw).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::{ensure_plan_declares_design_targets, planned_design_paths};

    #[test]
    fn extracts_normalized_design_targets_from_markdown() {
        let plan = r#"
- `design/02-crates.md`
- `design/runtime/pipeline.md` and `design/界面.md`
- duplicate `design/02-crates.md`
"#;

        assert_eq!(
            planned_design_paths(plan).into_iter().collect::<Vec<_>>(),
            vec![
                "design/02-crates.md".to_string(),
                "design/runtime/pipeline.md".to_string(),
                "design/界面.md".to_string(),
            ]
        );
    }

    #[test]
    fn ignores_non_targets_and_fenced_examples() {
        let plan = r#"
`redesign/not-a-target.md`
https://example.test/design/remote.md
design/prose-is-not-a-contract.md
[link](design/link-is-not-a-contract.md)
`design\windows.md`
`design/../escape.md`
`design/spec.MD`
`design/**/*.md`
`design.md`
`design/.md`
```text
design/example.md
```
"#;

        assert!(planned_design_paths(plan).is_empty());
    }

    #[test]
    fn confirmed_plan_requires_at_least_one_explicit_design_target() {
        let error = ensure_plan_declares_design_targets(
            "Update the relevant design docs, then implement the task.",
        )
        .unwrap_err();

        assert!(error.to_string().contains("inline-code workspace-relative"));
        ensure_plan_declares_design_targets("Update `design/任务.md`.").unwrap();
    }
}

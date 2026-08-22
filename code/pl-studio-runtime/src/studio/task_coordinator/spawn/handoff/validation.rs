//! 蓝图字段规范化 helper 与验证结果映射。

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};

use super::super::normalize_scope_hints;
use super::blueprint::{
    TaskExecutorBlueprint, TaskExecutorDependency, TaskExecutorEvidence, TaskExecutorTarget,
};

pub(super) fn normalize_required(value: &str, field: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{field} must not be empty")
    }
    Ok(value.to_string())
}

pub(super) fn normalize_identifier(value: &str, field: &str) -> Result<String> {
    let value = normalize_required(value, field)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("{field} `{value}` must contain only letters, digits, '-' or '_'")
    }
    Ok(value)
}

pub(super) fn normalize_required_list(
    values: Vec<String>,
    field: &str,
    require_non_empty: bool,
) -> Result<Vec<String>> {
    if require_non_empty && values.is_empty() {
        bail!("{field} must not be empty")
    }
    values
        .into_iter()
        .map(|value| normalize_required(&value, field))
        .collect()
}

pub(super) fn register_id(ids: &mut BTreeSet<String>, id: &str) -> Result<()> {
    if !ids.insert(id.to_string()) {
        bail!("duplicate executor blueprint id `{id}`")
    }
    Ok(())
}

pub(super) fn normalize_targets(targets: &mut [TaskExecutorTarget], field: &str) -> Result<()> {
    if targets.is_empty() {
        bail!("{field} must not be empty")
    }
    for target in targets {
        target.path = normalize_scope_hints(std::slice::from_ref(&target.path))?
            .into_iter()
            .next()
            .context("executor target path is missing")?;
        target.symbol = target
            .symbol
            .take()
            .map(|symbol| normalize_required(&symbol, "target.symbol"))
            .transpose()?;
    }
    Ok(())
}

pub(super) fn normalize_references(
    references: &mut [String],
    valid: &BTreeSet<String>,
    field: &str,
) -> Result<()> {
    if references.is_empty() {
        bail!("{field} must not be empty")
    }
    let mut seen = BTreeSet::new();
    for reference in references.iter_mut() {
        *reference = normalize_identifier(reference, field)?;
        if !valid.contains(reference) {
            bail!("{field} references unknown acceptance criterion `{reference}`")
        }
        if !seen.insert(reference.clone()) {
            bail!("{field} contains duplicate reference `{reference}`")
        }
    }
    Ok(())
}

pub(super) fn require_full_coverage(
    expected: &BTreeSet<String>,
    actual: &BTreeSet<String>,
    source: &str,
) -> Result<()> {
    let missing = expected.difference(actual).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "acceptance criteria missing {source} coverage: {}",
            missing.join(", ")
        )
    }
    Ok(())
}

pub(super) fn normalize_cwd(value: &str) -> Result<String> {
    let cwd = value.trim();
    if cwd == "." {
        return Ok(cwd.to_string());
    }
    normalize_scope_hints(&[cwd.to_string()])?
        .into_iter()
        .next()
        .context("verification command cwd is missing")
}

pub(super) fn normalize_dependencies(dependencies: &mut [TaskExecutorDependency]) -> Result<()> {
    for dependency in dependencies {
        dependency.kind = normalize_required(&dependency.kind, "dependencies.kind")?;
        dependency.id = normalize_required(&dependency.id, "dependencies.id")?;
        dependency.note = dependency
            .note
            .take()
            .map(|note| normalize_required(&note, "dependencies.note"))
            .transpose()?;
    }
    Ok(())
}

pub(super) fn normalize_evidence(evidence: &mut [TaskExecutorEvidence]) -> Result<()> {
    for item in evidence {
        item.path = normalize_scope_hints(std::slice::from_ref(&item.path))?
            .into_iter()
            .next()
            .context("executor evidence path is missing")?;
        item.symbol = item
            .symbol
            .take()
            .map(|symbol| normalize_required(&symbol, "evidence.symbol"))
            .transpose()?;
        item.content_hash = item
            .content_hash
            .take()
            .map(|hash| normalize_required(&hash, "evidence.contentHash"))
            .transpose()?;
        item.note = item
            .note
            .take()
            .map(|note| normalize_required(&note, "evidence.note"))
            .transpose()?;
    }
    Ok(())
}

pub(crate) fn verification_result_map<'a, T>(
    blueprint: &TaskExecutorBlueprint,
    results: impl IntoIterator<Item = (&'a str, T)>,
) -> Result<BTreeMap<String, T>> {
    let expected = blueprint.verification_ids().collect::<BTreeSet<_>>();
    let mut actual = BTreeMap::new();
    for (id, value) in results {
        if !expected.contains(id) {
            bail!("verification result references unknown check `{id}`")
        }
        if actual.insert(id.to_string(), value).is_some() {
            bail!("verification result repeats check `{id}`")
        }
    }
    let missing = expected
        .into_iter()
        .filter(|id| !actual.contains_key(*id))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "verification results are missing checks: {}",
            missing.join(", ")
        )
    }
    Ok(actual)
}

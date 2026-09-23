use anyhow::{Context, Result, bail};
use serde_norway::Value;
use std::fs;
use std::path::Path;

pub(crate) const CANONICAL_HOSTED_URL: &str = "https://pub.dev";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockfileChange {
    Unchanged,
    HostedUrlsOnly,
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

pub(crate) fn restore_optional(path: &Path, content: Option<&str>) -> Result<()> {
    match content {
        Some(content) => {
            fs::write(path, content)
                .with_context(|| format!("failed to restore {}", path.display()))?;
        }
        None if path.exists() => {
            fs::remove_file(path)
                .with_context(|| format!("failed to remove generated {}", path.display()))?;
        }
        None => {}
    }
    Ok(())
}

pub(crate) fn restore_canonical_optional(path: &Path, content: Option<&str>) -> Result<()> {
    restore_optional(path, content)?;
    rewrite_hosted_urls(path, CANONICAL_HOSTED_URL)
}

pub(crate) fn rewrite_hosted_urls(path: &Path, hosted_url: &str) -> Result<()> {
    let Some(content) = read_optional(path)? else {
        return Ok(());
    };
    let value: Value =
        serde_norway::from_str(&content).context("failed to parse pubspec.lock as YAML")?;
    let expected_hosted_packages = hosted_package_count(&value);
    let (rewritten, visited_hosted_packages) = rewrite_hosted_url_lines(&content, hosted_url);
    if visited_hosted_packages != expected_hosted_packages {
        bail!(
            "pubspec.lock hosted package layout is unsupported: expected {expected_hosted_packages}, found {visited_hosted_packages}"
        );
    }
    if rewritten == content {
        return Ok(());
    }
    fs::write(path, rewritten)
        .with_context(|| format!("failed to rewrite hosted URLs in {}", path.display()))
}

fn hosted_package_count(value: &Value) -> usize {
    let Value::Mapping(root) = value else {
        return 0;
    };
    let Some(Value::Mapping(packages)) = root.get("packages") else {
        return 0;
    };
    packages
        .values()
        .filter(|package| {
            let Value::Mapping(package) = package else {
                return false;
            };
            matches!(
                package.get("source"),
                Some(Value::String(source)) if source == "hosted"
            )
        })
        .count()
}

fn rewrite_hosted_url_lines(content: &str, hosted_url: &str) -> (String, usize) {
    let lines = content.split_inclusive('\n').collect::<Vec<_>>();
    let package_starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let body = line.trim_end_matches(['\r', '\n']);
            (body.starts_with("  ") && !body.starts_with("    ") && body.trim_end().ends_with(':'))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let mut rewritten = lines
        .iter()
        .map(|line| (*line).to_string())
        .collect::<Vec<_>>();
    let mut visited_hosted_packages = 0;

    for (offset, start) in package_starts.iter().copied().enumerate() {
        let end = package_starts
            .get(offset + 1)
            .copied()
            .unwrap_or(lines.len());
        if !lines[start..end]
            .iter()
            .any(|line| line.trim() == "source: hosted")
        {
            continue;
        }
        visited_hosted_packages += 1;
        if let Some((relative_index, line)) = lines[start..end]
            .iter()
            .enumerate()
            .find(|(_, line)| line.trim_start().starts_with("url:"))
        {
            let newline = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let indent_len = line.len() - line.trim_start().len();
            rewritten[start + relative_index] =
                format!("{}url: \"{hosted_url}\"{newline}", &line[..indent_len]);
        }
    }

    (rewritten.concat(), visited_hosted_packages)
}

pub(crate) fn classify_change(path: &Path, original: Option<&str>) -> Result<LockfileChange> {
    let current = read_optional(path)?;
    if current.as_deref() == original {
        return Ok(LockfileChange::Unchanged);
    }

    if normalized_lockfile(current.as_deref())? == normalized_lockfile(original)? {
        return Ok(LockfileChange::HostedUrlsOnly);
    }

    bail!(
        "flutter pub get changed pubspec.lock beyond hosted source URLs. \
         Run flutter pub get manually and review the lockfile before building release artifacts."
    )
}

fn set_hosted_urls(value: &mut Value, hosted_url: &str) -> bool {
    let Value::Mapping(root) = value else {
        return false;
    };
    let Some(Value::Mapping(packages)) = root.get_mut("packages") else {
        return false;
    };

    let mut changed = false;
    for package in packages.values_mut() {
        let Value::Mapping(package) = package else {
            continue;
        };
        let is_hosted = matches!(
            package.get("source"),
            Some(Value::String(source)) if source == "hosted"
        );
        if !is_hosted {
            continue;
        }
        let Some(Value::Mapping(description)) = package.get_mut("description") else {
            continue;
        };
        let Some(url) = description.get_mut("url") else {
            continue;
        };
        let replacement = Value::String(hosted_url.to_owned());
        if *url != replacement {
            *url = replacement;
            changed = true;
        }
    }
    changed
}

fn normalized_lockfile(content: Option<&str>) -> Result<Option<Value>> {
    let Some(content) = content else {
        return Ok(None);
    };
    let mut value: Value =
        serde_norway::from_str(content).context("failed to parse pubspec.lock as YAML")?;
    set_hosted_urls(&mut value, "<hosted-url>");
    Ok(Some(value))
}

use anyhow::{Context, Result, bail};
use serde_norway::Value;
use std::fs;
use std::path::Path;

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

pub(crate) fn rewrite_hosted_urls(path: &Path, hosted_url: &str) -> Result<()> {
    let Some(content) = read_optional(path)? else {
        return Ok(());
    };
    let mut value: Value =
        serde_norway::from_str(&content).context("failed to parse pubspec.lock as YAML")?;
    if !set_hosted_urls(&mut value, hosted_url) {
        return Ok(());
    }

    let content =
        serde_norway::to_string(&value).context("failed to serialize pubspec.lock as YAML")?;
    fs::write(path, content)
        .with_context(|| format!("failed to rewrite hosted URLs in {}", path.display()))
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
    normalize_urls(&mut value);
    Ok(Some(value))
}

fn normalize_urls(value: &mut Value) {
    match value {
        Value::Sequence(sequence) => {
            for item in sequence {
                normalize_urls(item);
            }
        }
        Value::Mapping(mapping) => {
            if let Some(url) = mapping.get_mut("url") {
                *url = Value::String("<hosted-url>".to_owned());
            }
            for child in mapping.values_mut() {
                normalize_urls(child);
            }
        }
        Value::Tagged(tagged) => normalize_urls(&mut tagged.value),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn classifies_hosted_url_only_changes() -> Result<()> {
        let path = temp_lockfile_path("hosted-url-only");
        let original = r#"
packages:
  async:
    description:
      name: async
      url: "https://pub.dev"
"#;
        let current = r#"
packages:
  async:
    description:
      name: async
      url: "https://mirror.example"
"#;
        fs::write(&path, current)?;
        assert_eq!(
            classify_change(&path, Some(original))?,
            LockfileChange::HostedUrlsOnly
        );
        let _ = fs::remove_file(path);
        Ok(())
    }

    #[test]
    fn rejects_non_url_changes() -> Result<()> {
        let path = temp_lockfile_path("non-url-change");
        let original = r#"
packages:
  async:
    version: "2.11.0"
"#;
        let current = r#"
packages:
  async:
    version: "2.12.0"
"#;
        fs::write(&path, current)?;
        assert!(classify_change(&path, Some(original)).is_err());
        let _ = fs::remove_file(path);
        Ok(())
    }

    #[test]
    fn rewrites_only_hosted_package_urls() -> Result<()> {
        let path = temp_lockfile_path("rewrite-hosted-urls");
        let original = r#"
packages:
  async:
    description:
      name: async
      url: "https://pub.dev"
    source: hosted
    version: "2.13.0"
  local_package:
    description:
      path: "../local_package"
      url: "https://example.invalid/metadata"
    source: path
    version: "1.0.0"
"#;
        fs::write(&path, original)?;

        rewrite_hosted_urls(&path, "https://mirror.example")?;

        let rewritten = fs::read_to_string(&path)?;
        let value: Value = serde_norway::from_str(&rewritten)?;
        assert_eq!(
            value["packages"]["async"]["description"]["url"],
            "https://mirror.example"
        );
        assert_eq!(
            value["packages"]["local_package"]["description"]["url"],
            "https://example.invalid/metadata"
        );
        assert_eq!(
            classify_change(&path, Some(original))?,
            LockfileChange::HostedUrlsOnly
        );
        let _ = fs::remove_file(path);
        Ok(())
    }

    fn temp_lockfile_path(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir().join(format!("pl-xtask-{name}-{nonce}.lock"))
    }
}

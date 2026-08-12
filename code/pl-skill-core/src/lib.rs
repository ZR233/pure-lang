use std::path::{Component, Path, PathBuf};

use gray_matter::{Matter, engine::YAML};
use serde::{Deserialize, Serialize};

pub const SKILL_FILE_NAME: &str = "SKILL.md";
pub const DEFAULT_ALLOWED_SUPPORT_DIRS: &[&str] = &["references", "templates", "scripts", "assets"];

pub type SkillCoreResult<T> = Result<T, SkillCoreError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SkillCoreError {
    message: String,
}

impl SkillCoreError {
    pub fn new(message: impl std::fmt::Display) -> Self {
        Self {
            message: message.to_string(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn into_message(self) -> String {
        self.message
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub metadata: SkillFrontmatterMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillFrontmatterMetadata {
    #[serde(default, rename = "short-description")]
    pub short_description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDocument {
    pub frontmatter: SkillFrontmatter,
    pub body: String,
}

pub fn parse_skill_document(content: &str) -> SkillCoreResult<SkillDocument> {
    let normalized = content.strip_prefix('\u{feff}').unwrap_or(content);
    let parsed = Matter::<YAML>::new()
        .parse::<SkillFrontmatter>(normalized)
        .map_err(|error| {
            SkillCoreError::new(format!("failed to parse skill frontmatter: {error}"))
        })?;
    let frontmatter = parsed
        .data
        .ok_or_else(|| SkillCoreError::new("skill must start with YAML frontmatter"))?;
    Ok(SkillDocument {
        frontmatter,
        body: parsed.content,
    })
}

pub fn parse_skill_frontmatter(content: &str) -> SkillCoreResult<SkillFrontmatter> {
    Ok(parse_skill_document(content)?.frontmatter)
}

pub fn sanitize_single_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn optional_single_line(raw: impl AsRef<str>) -> Option<String> {
    let value = sanitize_single_line(raw.as_ref());
    (!value.is_empty()).then_some(value)
}

pub fn required_single_line(value: Option<String>, field: &'static str) -> SkillCoreResult<String> {
    let value = value.ok_or_else(|| SkillCoreError::new(format!("missing field `{field}`")))?;
    let value = sanitize_single_line(&value);
    if value.is_empty() {
        Err(SkillCoreError::new(format!("missing field `{field}`")))
    } else {
        Ok(value)
    }
}

pub fn validate_char_len(value: &str, max: usize, field: &'static str) -> SkillCoreResult<()> {
    if value.chars().count() <= max {
        Ok(())
    } else {
        Err(SkillCoreError::new(format!(
            "invalid {field}: must be at most {max} characters"
        )))
    }
}

pub fn safe_relative_path(path: &str) -> SkillCoreResult<PathBuf> {
    let mut result = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SkillCoreError::new(format!(
                    "path must be relative and stay inside its root: {path}"
                )));
            }
        }
    }
    if result.as_os_str().is_empty() {
        return Err(SkillCoreError::new("path must not be empty"));
    }
    Ok(result)
}

pub fn support_file_path(path: &str, allowed_prefixes: &[&str]) -> SkillCoreResult<PathBuf> {
    let relative = safe_relative_path(path)?;
    let first = relative
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(part) => part.to_str(),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => None,
        })
        .ok_or_else(|| SkillCoreError::new("support file path must not be empty"))?;
    if !allowed_prefixes.contains(&first) {
        return Err(SkillCoreError::new(format!(
            "support file path must start with one of: {}",
            allowed_prefixes.join(", ")
        )));
    }
    Ok(relative)
}

pub fn resolve_support_file_path(
    skill_dir: &Path,
    relative: &str,
    allowed_prefixes: &[&str],
) -> SkillCoreResult<PathBuf> {
    let relative = support_file_path(relative, allowed_prefixes)?;
    Ok(skill_dir.join(relative))
}

pub fn resolve_canonical_support_file_path(
    skill_dir: &Path,
    relative: &str,
    allowed_prefixes: &[&str],
) -> SkillCoreResult<PathBuf> {
    let candidate = resolve_support_file_path(skill_dir, relative, allowed_prefixes)?;
    let skill_dir = canonicalize_existing(skill_dir, "skill directory")?;
    let canonical = canonicalize_existing(&candidate, "support file")?;
    if canonical.starts_with(&skill_dir) {
        Ok(canonical)
    } else {
        Err(SkillCoreError::new(format!(
            "support file path '{}' escapes the skill directory",
            candidate.display()
        )))
    }
}

fn canonicalize_existing(path: &Path, label: &str) -> SkillCoreResult<PathBuf> {
    std::fs::canonicalize(path).map_err(|error| {
        SkillCoreError::new(format!(
            "failed to resolve {label} '{}': {error}",
            path.display()
        ))
    })
}

pub fn normalized_platforms(platforms: Vec<String>) -> Vec<String> {
    platforms
        .into_iter()
        .map(|platform| normalize_platform(&platform))
        .filter(|platform| !platform.is_empty())
        .collect()
}

fn normalize_platform(platform: &str) -> String {
    match platform.trim().to_ascii_lowercase().as_str() {
        "win" | "windows" => "windows".to_string(),
        "mac" | "macos" | "darwin" => "macos".to_string(),
        "linux" => "linux".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pl-skill-core-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn parses_frontmatter_and_body() {
        let document = parse_skill_document(
            "---\nname: demo\ndescription: Do thing\nplatforms:\n  - mac\n---\nBody\n",
        )
        .unwrap();

        assert_eq!(
            document.frontmatter,
            SkillFrontmatter {
                name: "demo".to_string(),
                description: "Do thing".to_string(),
                category: None,
                platforms: vec!["mac".to_string()],
                metadata: SkillFrontmatterMetadata::default(),
            }
        );
        assert_eq!(document.body, "Body");
    }

    #[test]
    fn support_file_path_requires_allowed_prefix() {
        let error = support_file_path("notes/demo.md", DEFAULT_ALLOWED_SUPPORT_DIRS).unwrap_err();

        assert!(
            error
                .message()
                .contains("support file path must start with one of")
        );
    }

    #[test]
    fn support_file_path_rejects_parent_components() {
        let error =
            support_file_path("assets/../secret.txt", DEFAULT_ALLOWED_SUPPORT_DIRS).unwrap_err();

        assert!(error.message().contains("stay inside its root"));
    }

    #[test]
    fn canonical_support_file_rejects_symlink_escape() {
        let root = temp_dir("symlink");
        let outside = temp_dir("outside");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "secret").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("assets/secret.txt"))
            .unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(
            outside.join("secret.txt"),
            root.join("assets/secret.txt"),
        )
        .unwrap();

        let error = resolve_canonical_support_file_path(
            &root,
            "assets/secret.txt",
            DEFAULT_ALLOWED_SUPPORT_DIRS,
        )
        .unwrap_err();

        assert!(error.message().contains("escapes the skill directory"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}

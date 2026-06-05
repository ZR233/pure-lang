use std::fs;
use std::path::{Component, Path, PathBuf};

use pl_protocol::{PureError, Result};

use super::util::{category_path, normalized_platforms, safe_relative_path};
use super::{
    ALLOWED_SUPPORT_DIRS, SKILL_FILE_NAME, SkillFile, SkillFrontmatter, SkillMetadata,
    SkillSourceKind,
};

pub fn list_support_files(skill_dir: &Path) -> Result<Vec<SkillFile>> {
    let mut files = Vec::new();
    for dir in ALLOWED_SUPPORT_DIRS {
        let root = skill_dir.join(dir);
        if !root.exists() {
            continue;
        }
        collect_support_files(skill_dir, &root, &mut files)?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub fn read_skill_file(skill: &SkillMetadata, file_path: Option<&str>) -> Result<String> {
    let path = match file_path {
        Some(file_path) => {
            let relative = support_file_path(file_path)?;
            skill.path.join(relative)
        }
        None => skill.path.join(SKILL_FILE_NAME),
    };
    fs::read_to_string(&path).map_err(|error| PureError::ToolExecutionFailed {
        tool: "skill_view".to_string(),
        error: format!("failed to read skill file {}: {error}", path.display()),
    })
}

pub fn validate_skill_document(
    content: &str,
    expected_name: Option<&str>,
) -> Result<SkillMetadata> {
    let frontmatter = parse_frontmatter(content)?;
    super::validate_skill_name(&frontmatter.name)?;
    let description = frontmatter.description.trim();
    if description.is_empty() {
        return Err(PureError::ConfigError(
            "skill description must not be empty".to_string(),
        ));
    }
    if description.chars().count() > 1024 {
        return Err(PureError::ConfigError(
            "skill description must be at most 1024 characters".to_string(),
        ));
    }
    if let Some(expected_name) = expected_name
        && !frontmatter.name.eq_ignore_ascii_case(expected_name)
    {
        return Err(PureError::ConfigError(format!(
            "skill frontmatter name '{}' does not match target '{}'",
            frontmatter.name, expected_name
        )));
    }
    if let Some(category) = &frontmatter.category {
        let _ = category_path(category)?;
    }
    Ok(SkillMetadata {
        name: frontmatter.name,
        description: description.to_string(),
        category: frontmatter
            .category
            .map(|category| category.trim().to_string())
            .filter(|category| !category.is_empty()),
        platforms: normalized_platforms(frontmatter.platforms),
        source: SkillSourceKind::Project,
        path: PathBuf::new(),
    })
}

pub fn support_file_path(path: &str) -> Result<PathBuf> {
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
        .ok_or_else(|| PureError::ConfigError("support file path must not be empty".to_string()))?;
    if !ALLOWED_SUPPORT_DIRS.contains(&first) {
        return Err(PureError::ConfigError(format!(
            "support file path must start with one of: {}",
            ALLOWED_SUPPORT_DIRS.join(", ")
        )));
    }
    Ok(relative)
}

pub fn project_skill_dir_for_create(
    project_dir: &Path,
    name: &str,
    category: Option<&str>,
) -> Result<PathBuf> {
    super::validate_skill_name(name)?;
    let mut dir = project_dir.to_path_buf();
    if let Some(category) = category {
        dir = dir.join(category_path(category)?);
    }
    Ok(dir.join(name))
}

pub(super) fn metadata_from_file(
    skill_file: &Path,
    source_root: &Path,
    source: SkillSourceKind,
) -> Result<SkillMetadata> {
    let content = fs::read_to_string(skill_file).map_err(|error| {
        PureError::ConfigError(format!(
            "failed to read skill {}: {error}",
            skill_file.display()
        ))
    })?;
    let mut metadata = validate_skill_document(&content, None)?;
    let skill_dir = skill_file.parent().ok_or_else(|| {
        PureError::ConfigError(format!("invalid skill path: {}", skill_file.display()))
    })?;
    if metadata.category.is_none() {
        metadata.category = category_from_path(source_root, skill_dir);
    }
    metadata.source = source;
    metadata.path = skill_dir.to_path_buf();
    Ok(metadata)
}

pub(super) fn parse_frontmatter(content: &str) -> Result<SkillFrontmatter> {
    let normalized = content.strip_prefix('\u{feff}').unwrap_or(content);
    let Some(after_open) = normalized.strip_prefix("---") else {
        return Err(PureError::ConfigError(
            "skill must start with YAML frontmatter".to_string(),
        ));
    };
    let after_open = after_open
        .strip_prefix("\r\n")
        .or_else(|| after_open.strip_prefix('\n'))
        .ok_or_else(|| {
            PureError::ConfigError("skill frontmatter opener must be on its own line".to_string())
        })?;
    let mut frontmatter = String::new();
    for line in after_open.lines() {
        if line.trim() == "---" {
            return serde_norway::from_str::<SkillFrontmatter>(&frontmatter).map_err(|error| {
                PureError::ConfigError(format!("failed to parse skill frontmatter: {error}"))
            });
        }
        frontmatter.push_str(line);
        frontmatter.push('\n');
    }
    Err(PureError::ConfigError(
        "skill frontmatter is missing closing ---".to_string(),
    ))
}

pub(super) fn find_skill_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    find_skill_files_inner(root, 0, &mut files);
    files
}

fn find_skill_files_inner(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > super::MAX_SKILL_SCAN_DEPTH {
        return;
    }
    if dir.join(SKILL_FILE_NAME).is_file() {
        files.push(dir.join(SKILL_FILE_NAME));
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if !path.is_dir() || should_skip_dir(&path) {
            continue;
        }
        find_skill_files_inner(&path, depth + 1, files);
    }
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    name.starts_with('.')
        || matches!(
            name,
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "references"
                | "templates"
                | "scripts"
                | "assets"
        )
}

fn category_from_path(root: &Path, skill_dir: &Path) -> Option<String> {
    let relative = skill_dir.strip_prefix(root).ok()?;
    let mut components = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str().map(ToOwned::to_owned),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => None,
        })
        .collect::<Vec<_>>();
    if components.len() <= 1 {
        return None;
    }
    components.pop();
    Some(components.join("/"))
}

fn collect_support_files(skill_dir: &Path, dir: &Path, files: &mut Vec<SkillFile>) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|error| {
        PureError::ConfigError(format!(
            "failed to read support files in {}: {error}",
            dir.display()
        ))
    })?;
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_support_files(skill_dir, &path, files)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let relative = path.strip_prefix(skill_dir).map_err(|error| {
            PureError::ConfigError(format!("failed to resolve support file path: {error}"))
        })?;
        let metadata = path.metadata().map_err(|error| {
            PureError::ConfigError(format!(
                "failed to read support file metadata {}: {error}",
                path.display()
            ))
        })?;
        files.push(SkillFile {
            path: relative.to_string_lossy().replace('\\', "/"),
            bytes: metadata.len(),
        });
    }
    Ok(())
}

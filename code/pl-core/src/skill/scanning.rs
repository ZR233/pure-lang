use std::fs;
use std::path::{Component, Path, PathBuf};

use pl_protocol::{PureError, Result};

use crate::path_safety::{metadata_if_real, real_directory_entries, validate_existing_path};

use super::util::category_path;
use super::{
    ALLOWED_SUPPORT_DIRS, SKILL_FILE_NAME, SkillFile, SkillFrontmatter, SkillInvocationPolicy,
    SkillMetadata, SkillProviderId, SkillResourceBase, SkillSourceKind,
};

pub fn list_support_files(skill_dir: &Path) -> Result<Vec<SkillFile>> {
    let mut files = Vec::new();
    for dir in ALLOWED_SUPPORT_DIRS {
        let root = skill_dir.join(dir);
        let Some(metadata) =
            metadata_if_real(&root).map_err(|error| PureError::ConfigError(error.to_string()))?
        else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        collect_support_files(skill_dir, &root, &mut files)?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub struct SkillFileRead {
    pub file_path: String,
    pub is_main: bool,
    pub content: String,
}

pub fn read_skill_file(skill: &SkillMetadata, file_path: Option<&str>) -> Result<SkillFileRead> {
    let target = skill_file_selection(file_path);
    let path = match target {
        SkillFileSelection::Support(file_path) => {
            let relative = support_file_path(file_path)?;
            skill.path.join(relative)
        }
        SkillFileSelection::Main => skill.path.join(SKILL_FILE_NAME),
    };
    ensure_real_skill_path(&skill.path, &path)?;
    let content = fs::read_to_string(&path).map_err(|error| {
        let display_path = path.display();
        PureError::ToolExecutionFailed {
            tool: "skill_view".to_string(),
            error: format!("failed to read skill file {display_path}: {error}"),
        }
    })?;
    Ok(SkillFileRead {
        file_path: target.display_path().to_string(),
        is_main: target == SkillFileSelection::Main,
        content,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillFileSelection<'a> {
    Main,
    Support(&'a str),
}

impl<'a> SkillFileSelection<'a> {
    fn display_path(self) -> &'a str {
        match self {
            Self::Main => SKILL_FILE_NAME,
            Self::Support(path) => path,
        }
    }
}

fn skill_file_selection(file_path: Option<&str>) -> SkillFileSelection<'_> {
    let Some(trimmed) = file_path.map(str::trim).filter(|path| !path.is_empty()) else {
        return SkillFileSelection::Main;
    };
    let normalized = trimmed.replace('\\', "/");
    let normalized = normalized.trim_start_matches("./");
    if normalized.is_empty()
        || normalized == "."
        || normalized.eq_ignore_ascii_case(SKILL_FILE_NAME)
    {
        return SkillFileSelection::Main;
    }
    SkillFileSelection::Support(trimmed)
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
    validate_mode_metadata(&frontmatter)?;
    Ok(SkillMetadata {
        name: frontmatter.name,
        description: description.to_string(),
        category: frontmatter
            .category
            .map(|category| category.trim().to_string())
            .filter(|category| !category.is_empty()),
        platforms: pl_skill_core::normalized_platforms(frontmatter.platforms),
        source: SkillSourceKind::Project,
        path: PathBuf::new(),
        provider_id: SkillProviderId::new("local-filesystem")?,
        invocation: SkillInvocationPolicy {
            model_invocable: !frontmatter.disable_model_invocation,
            user_invocable: frontmatter.user_invocable,
        },
        resource_base: SkillResourceBase::Directory {
            path: PathBuf::new(),
        },
        mode: frontmatter.mode,
    })
}

pub fn support_file_path(path: &str) -> Result<PathBuf> {
    pl_skill_core::support_file_path(path, ALLOWED_SUPPORT_DIRS).map_err(skill_core_error)
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
        let display = skill_file.display();
        PureError::ConfigError(format!("invalid skill path: {display}"))
    })?;
    if metadata.category.is_none() {
        metadata.category = category_from_path(source_root, skill_dir);
    }
    metadata.source = source;
    metadata.path = skill_dir.to_path_buf();
    metadata.resource_base = SkillResourceBase::Directory {
        path: metadata.path.clone(),
    };
    Ok(metadata)
}

pub(super) fn parse_frontmatter(content: &str) -> Result<SkillFrontmatter> {
    pl_skill_core::parse_skill_frontmatter(content)
        .map(|frontmatter| SkillFrontmatter {
            name: frontmatter.name,
            description: frontmatter.description,
            category: frontmatter.category,
            platforms: frontmatter.platforms,
            disable_model_invocation: frontmatter.disable_model_invocation,
            user_invocable: frontmatter.user_invocable,
            mode: frontmatter.mode.map(|mode| super::ModeSkillMetadata {
                display_name: mode.display_name,
                order: mode.order,
            }),
        })
        .map_err(skill_core_error)
}

fn validate_mode_metadata(frontmatter: &SkillFrontmatter) -> Result<()> {
    let reserved = frontmatter.name.starts_with("mode.");
    match (reserved, &frontmatter.mode) {
        (true, None) => Err(PureError::ConfigError(format!(
            "reserved Mode Skill `{}` must declare mode metadata",
            frontmatter.name
        ))),
        (false, Some(_)) => Err(PureError::ConfigError(format!(
            "skill `{}` declares mode metadata without the reserved `mode.` prefix",
            frontmatter.name
        ))),
        (true, Some(mode)) => {
            let display_name = mode.display_name.trim();
            if display_name.is_empty() || display_name.chars().count() > 128 {
                return Err(PureError::ConfigError(
                    "mode display-name must contain 1 to 128 characters".to_string(),
                ));
            }
            if frontmatter.disable_model_invocation && !frontmatter.user_invocable {
                Ok(())
            } else {
                Err(PureError::ConfigError(format!(
                    "Mode Skill `{}` must set disable-model-invocation: true and user-invocable: false",
                    frontmatter.name
                )))
            }
        }
        (false, None) => Ok(()),
    }
}

pub(super) struct SkillFileScan {
    pub files: Vec<PathBuf>,
    pub complete: bool,
    pub warnings: Vec<String>,
}

pub(super) fn scan_skill_files(root: &Path) -> SkillFileScan {
    let mut files = Vec::new();
    let root_metadata = match metadata_if_real(root) {
        Ok(metadata) => metadata,
        Err(error) => {
            return SkillFileScan {
                files,
                complete: false,
                warnings: vec![format!(
                    "failed to inspect skill root {}: {error}",
                    root.display()
                )],
            };
        }
    };
    if !root_metadata.is_some_and(|metadata| metadata.is_dir()) {
        return SkillFileScan {
            files,
            complete: true,
            warnings: Vec::new(),
        };
    }
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .max_depth(Some(super::MAX_SKILL_SCAN_DEPTH + 1))
        .hidden(true)
        .follow_links(false)
        .require_git(false)
        .sort_by_file_name(std::cmp::Ord::cmp)
        .filter_entry(|entry| {
            entry.depth() == 0
                || entry
                    .path()
                    .file_name()
                    .is_some_and(|name| name == SKILL_FILE_NAME)
                || !should_skip_dir(entry.path())
        });
    let mut complete = true;
    let mut warnings = Vec::new();
    for entry in builder.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                complete = false;
                warnings.push(format!(
                    "failed to scan skill root {}: {error}",
                    root.display()
                ));
                continue;
            }
        };
        if entry.depth() == 0 || entry.file_name() != SKILL_FILE_NAME {
            continue;
        }
        let path = entry.into_path();
        match metadata_if_real(&path) {
            Ok(Some(metadata)) if metadata.is_file() => files.push(path),
            Ok(_) => {}
            Err(error) => {
                complete = false;
                warnings.push(format!(
                    "failed to inspect skill file {}: {error}",
                    path.display()
                ));
            }
        }
    }
    files.sort();
    files.dedup();
    let mut top_level_skills = Vec::new();
    for skill_file in files {
        let mut nested = false;
        if let Some(mut ancestor) = skill_file.parent().and_then(|directory| directory.parent()) {
            while ancestor.starts_with(root) && ancestor != root {
                let ancestor_skill = ancestor.join(SKILL_FILE_NAME);
                match metadata_if_real(&ancestor_skill) {
                    Ok(Some(metadata)) if metadata.is_file() => {
                        nested = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        complete = false;
                        warnings.push(format!(
                            "failed to inspect ancestor skill file {}: {error}",
                            ancestor_skill.display()
                        ));
                    }
                }
                let Some(parent) = ancestor.parent() else {
                    break;
                };
                ancestor = parent;
            }
        }
        if !nested {
            top_level_skills.push(skill_file);
        }
    }
    SkillFileScan {
        files: top_level_skills,
        complete,
        warnings,
    }
}

pub(super) fn find_skill_files(root: &Path) -> Vec<PathBuf> {
    scan_skill_files(root).files
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

fn skill_core_error(error: pl_skill_core::SkillCoreError) -> PureError {
    PureError::ConfigError(error.into_message())
}

fn collect_support_files(skill_dir: &Path, dir: &Path, files: &mut Vec<SkillFile>) -> Result<()> {
    let entries = real_directory_entries(dir).map_err(|error| {
        PureError::ConfigError(format!(
            "failed to read support files in {}: {error}",
            dir.display()
        ))
    })?;
    for path in entries {
        let Some(metadata) =
            metadata_if_real(&path).map_err(|error| PureError::ConfigError(error.to_string()))?
        else {
            continue;
        };
        if metadata.is_dir() {
            collect_support_files(skill_dir, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path.strip_prefix(skill_dir).map_err(|error| {
            PureError::ConfigError(format!("failed to resolve support file path: {error}"))
        })?;
        files.push(SkillFile {
            path: relative.to_string_lossy().replace('\\', "/"),
            bytes: metadata.len(),
        });
    }
    Ok(())
}

fn ensure_real_skill_path(skill_dir: &Path, path: &Path) -> Result<()> {
    let root_metadata = metadata_if_real(skill_dir)
        .map_err(|error| PureError::ToolExecutionFailed {
            tool: "skill_view".to_string(),
            error: error.to_string(),
        })?
        .ok_or_else(|| PureError::ToolExecutionFailed {
            tool: "skill_view".to_string(),
            error: format!(
                "skill directory is a symbolic link or Windows reparse point: {}",
                skill_dir.display()
            ),
        })?;
    if !root_metadata.is_dir() {
        return Err(PureError::ToolExecutionFailed {
            tool: "skill_view".to_string(),
            error: format!("skill path is not a directory: {}", skill_dir.display()),
        });
    }
    validate_existing_path(skill_dir, path).map_err(|error| PureError::ToolExecutionFailed {
        tool: "skill_view".to_string(),
        error: error.to_string(),
    })
}

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

use crate::config::SkillsConfig;

const SKILL_FILE_NAME: &str = "SKILL.md";
const USAGE_FILE_NAME: &str = ".usage.json";
const MAX_SKILL_SCAN_DEPTH: usize = 5;
const ALLOWED_SUPPORT_DIRS: &[&str] = &["references", "templates", "scripts", "assets"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkillSourceKind {
    Project,
    User,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub platforms: Vec<String>,
    pub source: SkillSourceKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillFile {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalog {
    pub project_dir: PathBuf,
    pub skills: Vec<SkillMetadata>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsage {
    pub created_by: String,
    pub views: u64,
    pub uses: u64,
    pub patches: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_viewed_at: Option<i64>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    platforms: Vec<String>,
}

#[derive(Debug, Clone)]
struct SkillCandidate {
    metadata: SkillMetadata,
    priority: u8,
}

impl SkillCatalog {
    pub fn discover(workspace_root: &Path, config: &SkillsConfig) -> Result<Self> {
        let project_dir = project_skills_dir(workspace_root, config)?;
        let mut warnings = Vec::new();
        let mut by_name: BTreeMap<String, SkillCandidate> = BTreeMap::new();
        let disabled = config
            .disabled
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();

        let sources = skill_sources(workspace_root, config)?;
        for source in sources {
            if !source.root.exists() {
                continue;
            }
            let files = find_skill_files(&source.root);
            for skill_file in files {
                match metadata_from_file(&skill_file, &source.root, source.kind) {
                    Ok(metadata) => {
                        if disabled.contains(&metadata.name.to_ascii_lowercase())
                            || !platform_matches(&metadata.platforms)
                        {
                            continue;
                        }
                        let key = metadata.name.to_ascii_lowercase();
                        let replace = by_name
                            .get(&key)
                            .is_none_or(|existing| source.priority < existing.priority);
                        if replace {
                            by_name.insert(
                                key,
                                SkillCandidate {
                                    metadata,
                                    priority: source.priority,
                                },
                            );
                        }
                    }
                    Err(error) => warnings.push(error.to_string()),
                }
            }
        }

        Ok(Self {
            project_dir,
            skills: by_name
                .into_values()
                .map(|candidate| candidate.metadata)
                .collect(),
            warnings,
        })
    }

    pub fn find(&self, name: &str) -> Option<&SkillMetadata> {
        self.skills
            .iter()
            .find(|skill| skill.name.eq_ignore_ascii_case(name))
    }

    pub fn project_skill(&self, name: &str) -> Option<&SkillMetadata> {
        self.find(name)
            .filter(|skill| skill.source == SkillSourceKind::Project)
    }
}

impl SkillUsage {
    pub fn agent_created(now: i64) -> Self {
        Self {
            created_by: "agent".to_string(),
            views: 0,
            uses: 0,
            patches: 0,
            created_at: now,
            updated_at: now,
            last_viewed_at: None,
            pinned: false,
        }
    }
}

pub fn validate_skills_config(config: &SkillsConfig) -> Result<()> {
    if config.project_dir.trim().is_empty() {
        return Err(PureError::ConfigError(
            "skills.project_dir must not be empty".to_string(),
        ));
    }
    if safe_relative_path(&config.project_dir).is_err() {
        return Err(PureError::ConfigError(
            "skills.project_dir must be a relative path inside the workspace".to_string(),
        ));
    }
    if config.user_dir.trim().is_empty() {
        return Err(PureError::ConfigError(
            "skills.user_dir must not be empty".to_string(),
        ));
    }
    for disabled in &config.disabled {
        validate_skill_name(disabled)?;
    }
    Ok(())
}

pub fn project_skills_dir(workspace_root: &Path, config: &SkillsConfig) -> Result<PathBuf> {
    let relative = safe_relative_path(&config.project_dir)?;
    Ok(workspace_root.join(relative))
}

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
    validate_skill_name(&frontmatter.name)?;
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

pub fn validate_skill_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(PureError::ConfigError(
            "skill name must not be empty".to_string(),
        ));
    }
    if trimmed.chars().count() > 64 {
        return Err(PureError::ConfigError(
            "skill name must be at most 64 characters".to_string(),
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(PureError::ConfigError(format!(
            "skill name contains unsupported characters: {trimmed}"
        )));
    }
    Ok(())
}

pub fn project_skill_dir_for_create(
    project_dir: &Path,
    name: &str,
    category: Option<&str>,
) -> Result<PathBuf> {
    validate_skill_name(name)?;
    let mut dir = project_dir.to_path_buf();
    if let Some(category) = category {
        dir = dir.join(category_path(category)?);
    }
    Ok(dir.join(name))
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

pub fn bump_project_view(skill: &SkillMetadata) -> Result<()> {
    if skill.source != SkillSourceKind::Project {
        return Ok(());
    }
    let now = unix_seconds();
    let mut usage = load_usage(&skill.path).unwrap_or_else(|| SkillUsage::agent_created(now));
    usage.views += 1;
    usage.uses += 1;
    usage.updated_at = now;
    usage.last_viewed_at = Some(now);
    save_usage(&skill.path, &usage)
}

pub fn mark_project_skill_created(skill_dir: &Path) -> Result<()> {
    save_usage(skill_dir, &SkillUsage::agent_created(unix_seconds()))
}

pub fn bump_project_patch(skill_dir: &Path) -> Result<()> {
    let now = unix_seconds();
    let mut usage = load_usage(skill_dir).unwrap_or_else(|| SkillUsage::agent_created(now));
    usage.patches += 1;
    usage.updated_at = now;
    save_usage(skill_dir, &usage)
}

pub fn build_skills_prompt(workspace_root: &Path, config: &SkillsConfig) -> Result<Option<String>> {
    if !config.enabled {
        return Ok(None);
    }
    let catalog = SkillCatalog::discover(workspace_root, config)?;
    if catalog.skills.is_empty() {
        return Ok(Some(
            "# Skills\n当前项目未发现可用 skills。完成可复用流程后，可用 `skill_manage` 写入项目 `skills/` 目录。".to_string(),
        ));
    }

    let mut prompt = String::from(
        "# Skills\n可用 skills 索引如下。任务明显匹配某个 skill 时，必须先调用 `skill_view(name)` 读取完整内容，再继续执行。\n\n",
    );
    for skill in &catalog.skills {
        let category = skill.category.as_deref().unwrap_or("uncategorized");
        prompt.push_str(&format!(
            "- `{}` [{} / {:?}]: {}\n",
            skill.name, category, skill.source, skill.description
        ));
    }
    prompt.push_str(
        "\n完成复杂任务、修复非平凡问题或发现可复用项目流程后，优先用 `skill_manage` 修补已有项目 skill；没有合适 skill 时创建新的项目 skill。不要记录一次性任务、瞬时环境失败或纯用户私密偏好。",
    );
    if !catalog.warnings.is_empty() {
        prompt.push_str("\n\n发现部分 skill 失败：\n");
        for warning in catalog.warnings.iter().take(5) {
            prompt.push_str(&format!("- {warning}\n"));
        }
    }
    Ok(Some(prompt))
}

#[derive(Debug, Clone)]
struct SkillSource {
    root: PathBuf,
    kind: SkillSourceKind,
    priority: u8,
}

fn skill_sources(workspace_root: &Path, config: &SkillsConfig) -> Result<Vec<SkillSource>> {
    let mut sources = Vec::new();
    sources.push(SkillSource {
        root: project_skills_dir(workspace_root, config)?,
        kind: SkillSourceKind::Project,
        priority: 0,
    });
    sources.push(SkillSource {
        root: expand_home(&config.user_dir)?,
        kind: SkillSourceKind::User,
        priority: 1,
    });
    for external_dir in &config.external_dirs {
        sources.push(SkillSource {
            root: expand_home(external_dir)?,
            kind: SkillSourceKind::External,
            priority: 2,
        });
    }
    Ok(sources)
}

fn metadata_from_file(
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

fn parse_frontmatter(content: &str) -> Result<SkillFrontmatter> {
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

fn find_skill_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    find_skill_files_inner(root, 0, &mut files);
    files
}

fn find_skill_files_inner(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > MAX_SKILL_SCAN_DEPTH {
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

fn safe_relative_path(path: &str) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PureError::ConfigError(format!(
                    "path must be relative and stay inside its root: {path}"
                )));
            }
        }
    }
    if result.as_os_str().is_empty() {
        return Err(PureError::ConfigError("path must not be empty".to_string()));
    }
    Ok(result)
}

fn category_path(category: &str) -> Result<PathBuf> {
    let relative = safe_relative_path(category.trim())?;
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(PureError::ConfigError(
                "skill category must contain normal path components".to_string(),
            ));
        };
        let Some(part) = part.to_str() else {
            return Err(PureError::ConfigError(
                "skill category must be valid UTF-8".to_string(),
            ));
        };
        if !part
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        {
            return Err(PureError::ConfigError(format!(
                "skill category contains unsupported characters: {category}"
            )));
        }
    }
    Ok(relative)
}

fn expand_home(path: &str) -> Result<PathBuf> {
    if path == "~" {
        return user_home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        return Ok(user_home_dir()?.join(rest));
    }
    Ok(PathBuf::from(path))
}

fn user_home_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];

    HOME_VARS
        .iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| PureError::ConfigError("could not resolve user home directory".to_string()))
}

fn normalized_platforms(platforms: Vec<String>) -> Vec<String> {
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

fn platform_matches(platforms: &[String]) -> bool {
    platforms.is_empty()
        || platforms
            .iter()
            .any(|platform| platform == current_platform())
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn load_usage(skill_dir: &Path) -> Option<SkillUsage> {
    fs::read_to_string(skill_dir.join(USAGE_FILE_NAME))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

fn save_usage(skill_dir: &Path, usage: &SkillUsage) -> Result<()> {
    fs::create_dir_all(skill_dir)?;
    let content = serde_json::to_string_pretty(usage).map_err(|error| {
        PureError::ConfigError(format!("failed to serialize skill usage: {error}"))
    })?;
    fs::write(skill_dir.join(USAGE_FILE_NAME), content)?;
    Ok(())
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("pure-skill-{name}-{stamp}"))
    }

    fn write_skill(dir: &Path, name: &str, description: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join(SKILL_FILE_NAME),
            format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n"),
        )
        .unwrap();
    }

    #[test]
    fn parses_valid_frontmatter() {
        let content =
            "---\nname: rust-flow\ndescription: Rust flow\nplatforms: [windows]\n---\nBody";

        let metadata = validate_skill_document(content, Some("rust-flow")).unwrap();

        assert_eq!(metadata.name, "rust-flow");
        assert_eq!(metadata.description, "Rust flow");
        assert_eq!(metadata.platforms, vec!["windows".to_string()]);
    }

    #[test]
    fn rejects_missing_frontmatter() {
        let error = validate_skill_document("# Nope", None)
            .unwrap_err()
            .to_string();

        assert!(error.contains("frontmatter"));
    }

    #[test]
    fn project_source_shadows_user_and_external() {
        let workspace = temp_dir("shadow-workspace");
        let user = temp_dir("shadow-user");
        let external = temp_dir("shadow-external");
        write_skill(
            &workspace.join("skills").join("shared"),
            "shared",
            "project",
        );
        write_skill(&user.join("shared"), "shared", "user");
        write_skill(&external.join("shared"), "shared", "external");
        let mut config = SkillsConfig {
            user_dir: user.to_string_lossy().to_string(),
            ..SkillsConfig::default()
        };
        config
            .external_dirs
            .push(external.to_string_lossy().to_string());

        let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

        assert_eq!(catalog.skills.len(), 1);
        assert_eq!(catalog.skills[0].description, "project");
        assert_eq!(catalog.skills[0].source, SkillSourceKind::Project);
        fs::remove_dir_all(workspace).unwrap();
        fs::remove_dir_all(user).unwrap();
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn disabled_skills_are_filtered() {
        let workspace = temp_dir("disabled");
        write_skill(&workspace.join("skills").join("hidden"), "hidden", "hidden");
        let config = SkillsConfig {
            disabled: vec!["hidden".to_string()],
            ..SkillsConfig::default()
        };

        let catalog = SkillCatalog::discover(&workspace, &config).unwrap();

        assert!(catalog.skills.is_empty());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn support_file_rejects_traversal() {
        let error = support_file_path("../AGENTS.md").unwrap_err().to_string();

        assert!(error.contains("relative"));
    }

    #[test]
    fn support_file_requires_allowed_directory() {
        let error = support_file_path("notes/file.md").unwrap_err().to_string();

        assert!(error.contains("support file path"));
    }
}

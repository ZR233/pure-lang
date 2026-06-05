mod catalog;
mod scanning;
mod system;
mod util;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

use crate::config::SkillsConfig;

pub use catalog::build_skills_prompt;
pub use scanning::{
    list_support_files, project_skill_dir_for_create, read_skill_file, support_file_path,
    validate_skill_document,
};

pub const SKILL_FILE_NAME: &str = "SKILL.md";
pub const USAGE_FILE_NAME: &str = ".usage.json";
const SYSTEM_MARKER_FILE_NAME: &str = ".pure-system-skills.marker";
const SYSTEM_MARKER_SALT: &str = "v1";
const MAX_SKILL_SCAN_DEPTH: usize = 5;
const ALLOWED_SUPPORT_DIRS: &[&str] = &["references", "templates", "scripts", "assets"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkillSourceKind {
    Project,
    User,
    System,
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

#[derive(Debug, Clone)]
struct SkillSource {
    root: PathBuf,
    kind: SkillSourceKind,
    priority: u8,
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
    if util::safe_relative_path(&config.project_dir).is_err() {
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
    let relative = util::safe_relative_path(&config.project_dir)?;
    Ok(workspace_root.join(relative))
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

pub fn bump_project_view(skill: &SkillMetadata) -> Result<()> {
    if skill.source != SkillSourceKind::Project {
        return Ok(());
    }
    let now = util::unix_seconds();
    let mut usage = util::load_usage(&skill.path).unwrap_or_else(|| SkillUsage::agent_created(now));
    usage.views += 1;
    usage.uses += 1;
    usage.updated_at = now;
    usage.last_viewed_at = Some(now);
    util::save_usage(&skill.path, &usage)
}

pub fn mark_project_skill_created(skill_dir: &Path) -> Result<()> {
    util::save_usage(skill_dir, &SkillUsage::agent_created(util::unix_seconds()))
}

pub fn bump_project_patch(skill_dir: &Path) -> Result<()> {
    let now = util::unix_seconds();
    let mut usage = util::load_usage(skill_dir).unwrap_or_else(|| SkillUsage::agent_created(now));
    usage.patches += 1;
    usage.updated_at = now;
    util::save_usage(skill_dir, &usage)
}

mod catalog;
mod provider;
mod scanning;
mod selection;
mod util;

use std::path::{Path, PathBuf};

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

use crate::config::SkillsConfig;

pub(crate) use catalog::build_skill_suggestions_from_catalog;
pub use catalog::{build_skills_prompt, build_skills_prompt_from_catalog};
pub use provider::{
    BUILTIN_MODE_IDS, BUILTIN_MODE_PROVIDER_ID, FileSystemSkillProvider, FrozenSkillCatalog,
    SkillCandidate, SkillDefinition, SkillDirectorySource, SkillInvocationPolicy,
    SkillLoadInvocation, SkillProvider, SkillProviderId, SkillProviderInvalidator,
    SkillProviderObservation, SkillProviderRegistration, SkillProviderRequest, SkillRegistry,
    SkillResourceBase, SkillSummary, SkillUserInvocationLoad, discover_local_skills,
    local_skill_registry,
};
pub use scanning::{
    list_support_files, project_skill_dir_for_create, read_skill_file, support_file_path,
    validate_skill_document,
};
pub use selection::{SkillSelection, SkillSelectionRequest, SkillSelector};

pub const SKILL_FILE_NAME: &str = "SKILL.md";
pub const USAGE_FILE_NAME: &str = ".usage.json";
const MAX_SKILL_SCAN_DEPTH: usize = 5;
const ALLOWED_SUPPORT_DIRS: &[&str] = &["references", "templates", "scripts", "assets"];

/// Resolves the configured read-only user Skills directory, including `~` expansion.
///
/// # Errors
///
/// Returns an error when a home-relative path is configured and the host home
/// directory cannot be resolved.
pub fn resolve_user_skills_dir(config: &SkillsConfig) -> Result<PathBuf> {
    util::expand_home(&config.user_dir)
}

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
    #[serde(skip)]
    pub path: PathBuf,
    pub provider_id: SkillProviderId,
    pub invocation: SkillInvocationPolicy,
    pub resource_base: SkillResourceBase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ModeSkillMetadata>,
}

/// 模式选择器需要的稳定 Skill 元数据。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModeSkillMetadata {
    pub display_name: String,
    pub order: i32,
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
    /// 普通、可按需调用的 Skills；永远不包含 Mode Skill。
    pub skills: Vec<SkillMetadata>,
    /// 预加载模式目录；完整 name 即稳定 ModeId。
    #[serde(default)]
    pub modes: Vec<SkillMetadata>,
    pub warnings: Vec<String>,
    #[serde(default = "default_complete")]
    pub complete: bool,
}

/// Serializable Provider-neutral catalog projection for UI and HTTP consumers.
pub type SkillCatalogSnapshot = SkillCatalog;

impl From<SkillMetadata> for SkillSummary {
    fn from(metadata: SkillMetadata) -> Self {
        Self {
            name: metadata.name,
            description: metadata.description,
            category: metadata.category,
            platforms: metadata.platforms,
            source: metadata.source,
            provider_id: metadata.provider_id,
            invocation: metadata.invocation,
            resource_base: metadata.resource_base,
            mode: metadata.mode,
        }
    }
}

impl From<SkillSummary> for SkillMetadata {
    fn from(summary: SkillSummary) -> Self {
        let path = match &summary.resource_base {
            SkillResourceBase::Directory { path } => path.clone(),
            SkillResourceBase::Url { .. } | SkillResourceBase::Opaque { .. } => PathBuf::new(),
        };
        Self {
            name: summary.name,
            description: summary.description,
            category: summary.category,
            platforms: summary.platforms,
            source: summary.source,
            path,
            provider_id: summary.provider_id,
            invocation: summary.invocation,
            resource_base: summary.resource_base,
            mode: summary.mode,
        }
    }
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
    #[serde(default)]
    disable_model_invocation: bool,
    #[serde(default = "default_user_invocable")]
    user_invocable: bool,
    #[serde(default)]
    mode: Option<ModeSkillMetadata>,
}

#[derive(Debug, Clone)]
pub(super) struct SkillSource {
    pub(super) root: PathBuf,
    pub(super) kind: SkillSourceKind,
    pub(super) priority: u8,
}

const fn default_user_invocable() -> bool {
    true
}

const fn default_complete() -> bool {
    true
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
    if !workspace_root.exists() {
        return Ok(workspace_root.join(relative));
    }
    crate::tool::ToolPathPolicy::new(workspace_root.to_path_buf(), false, "skills")?
        .resolve_for_write(&config.project_dir)
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

pub fn bump_project_view(project_dir: &Path, skill: &SkillMetadata) -> Result<()> {
    if skill.source != SkillSourceKind::Project {
        return Ok(());
    }
    util::validate_usage_write(project_dir, &skill.path)?;
    let now = crate::time::unix_seconds();
    let mut usage =
        util::load_usage(&skill.path)?.unwrap_or_else(|| SkillUsage::agent_created(now));
    usage.views += 1;
    usage.uses += 1;
    usage.updated_at = now;
    usage.last_viewed_at = Some(now);
    util::save_usage(project_dir, &skill.path, &usage)
}

pub fn mark_project_skill_created(project_dir: &Path, skill_dir: &Path) -> Result<()> {
    util::save_usage(
        project_dir,
        skill_dir,
        &SkillUsage::agent_created(crate::time::unix_seconds()),
    )
}

pub fn bump_project_patch(project_dir: &Path, skill_dir: &Path) -> Result<()> {
    util::validate_usage_write(project_dir, skill_dir)?;
    let now = crate::time::unix_seconds();
    let mut usage = util::load_usage(skill_dir)?.unwrap_or_else(|| SkillUsage::agent_created(now));
    usage.patches += 1;
    usage.updated_at = now;
    util::save_usage(project_dir, skill_dir, &usage)
}

#[cfg(test)]
mod unit_tests;

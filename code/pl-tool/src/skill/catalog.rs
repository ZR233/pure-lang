use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use pl_protocol::{PureError, Result};

use super::scanning::{find_skill_files, metadata_from_file};
use super::util::platform_matches;
use super::{
    SkillCatalog, SkillMetadata, SkillSelectionRequest, SkillSelector, SkillSource, SkillSourceKind,
};
use crate::skill::SkillsConfig;

impl SkillCatalog {
    /// Discovers the effective Skills catalog for one workspace.
    ///
    /// Studio passes its product-owned system directory explicitly. Product-neutral
    /// callers pass `None` and never infer a system directory from `user_dir`. Both
    /// use the platform user home's `.agents/skills` compatibility directory.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured source path cannot be resolved or the
    /// project Skills directory escapes the workspace. An unavailable platform
    /// user home only omits the optional Agents compatibility source.
    pub fn discover(
        workspace_root: &Path,
        config: &SkillsConfig,
        system_dir: Option<&Path>,
    ) -> Result<Self> {
        let sources = skill_sources(workspace_root, config, system_dir)?;
        Self::discover_from_sources(workspace_root, config, sources)
    }

    fn discover_from_sources(
        workspace_root: &Path,
        config: &SkillsConfig,
        sources: Vec<SkillSource>,
    ) -> Result<Self> {
        let project_dir = super::project_skills_dir(workspace_root, config)?;
        let mut warnings = Vec::new();
        let mut by_name: BTreeMap<String, (SkillMetadata, u16)> = BTreeMap::new();
        let disabled = config
            .disabled
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();

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
                            .is_none_or(|existing| source.priority < existing.1);
                        if replace {
                            by_name.insert(key, (metadata, source.priority));
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
                .map(|(metadata, _)| metadata)
                .collect(),
            warnings,
            complete: true,
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

/// Resolves the ordered local read-only Skill sources shared by local and remote workspaces.
///
/// Sources are returned in winner priority order: the configured user directory, the platform
/// user's `.agents/skills` compatibility directory when distinct, the enabled Studio system
/// directory, and configured external directories. A missing platform user home only omits the
/// optional compatibility source.
///
/// # Errors
///
/// Returns an error when the configured user directory or an external directory cannot be
/// resolved.
pub fn resolve_local_readonly_skill_sources(
    config: &SkillsConfig,
    system_dir: Option<&Path>,
) -> Result<Vec<super::SkillDirectorySource>> {
    let agents_user_dir = super::util::agents_user_skills_dir().ok();
    resolve_local_readonly_skill_sources_with_agents_user_dir(
        config,
        system_dir,
        agents_user_dir.as_deref(),
    )
}

/// Builds the Skills prompt using an optional, explicitly supplied system source.
///
/// # Errors
///
/// Returns an error when catalog discovery fails.
pub fn build_skills_prompt(
    workspace_root: &Path,
    config: &SkillsConfig,
    system_dir: Option<&Path>,
) -> Result<Option<String>> {
    if !config.enabled {
        return Ok(None);
    }
    let catalog = SkillCatalog::discover(workspace_root, config, system_dir)?;
    Ok(Some(build_skills_prompt_from_catalog(&catalog)))
}

pub fn build_skills_prompt_from_catalog(catalog: &SkillCatalog) -> String {
    let mut model_skills = catalog
        .skills
        .iter()
        .filter(|skill| skill.invocation.model_invocable)
        .collect::<Vec<_>>();
    model_skills.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });
    if model_skills.is_empty() {
        return "# Skills\n当前项目未发现可用 skills。完成可复用流程后，可用 `skill_manage` 写入项目 `.agents/skills/` 目录。".to_string();
    }

    let mut prompt = String::from(
        "# Skills\n可用 skills 索引如下。任务明显匹配某个 skill 时，必须先调用 `skill_view(name)` 读取完整内容，再继续执行。\n\n",
    );
    for skill in model_skills {
        prompt.push_str(&format!(
            "- `{}`: {}\n",
            pl_skill_core::sanitize_single_line(&skill.name),
            model_description(&skill.description)
        ));
    }
    prompt.push_str(
        "\nSystem/User/External skills 是只读来源。完成复杂任务、修复非平凡问题或发现可复用项目流程后，优先用 `skill_manage` 修补已有项目 skill；没有合适 skill 时创建新的项目 skill。不要记录一次性任务、瞬时环境失败或纯用户私密偏好。",
    );
    prompt
}

pub fn build_skill_suggestions_from_catalog(
    catalog: &SkillCatalog,
    query: &str,
    excluded_names: &[String],
) -> Option<String> {
    let selection = SkillSelector.select(
        &catalog.skills,
        SkillSelectionRequest {
            query,
            limit: 5,
            category: None,
            excluded_names,
            model_invocable_only: true,
        },
    );
    if selection.matches.is_empty() {
        return None;
    }

    let mut prompt = String::from(
        "<skill_suggestions>\n以下 skills 与当前任务的 name 或 description 存在确定性词法匹配；这些只是摘要，不代表正文已经加载：\n",
    );
    for skill in selection.matches {
        prompt.push_str(&format!(
            "- `{}`: {}\n",
            pl_skill_core::sanitize_single_line(&skill.name),
            model_description(&skill.description)
        ));
    }
    prompt.push_str(
        "如需使用其中某个 skill，必须先以精确 name 调用 `skill_view`。已由用户直接加载的 skill 不要重复调用。\n</skill_suggestions>",
    );
    Some(prompt)
}

fn model_description(description: &str) -> String {
    const MAX_CHARS: usize = 500;
    const ELLIPSIS_CHARS: usize = 3;

    let normalized = pl_skill_core::sanitize_single_line(description);
    if normalized.chars().count() <= MAX_CHARS {
        return normalized;
    }
    let mut truncated = normalized
        .chars()
        .take(MAX_CHARS - ELLIPSIS_CHARS)
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

pub(super) fn skill_sources(
    workspace_root: &Path,
    config: &SkillsConfig,
    system_dir: Option<&Path>,
) -> Result<Vec<SkillSource>> {
    let readonly_sources = resolve_local_readonly_skill_sources(config, system_dir)?;
    skill_sources_from_readonly(workspace_root, config, readonly_sources)
}

fn skill_sources_from_readonly(
    workspace_root: &Path,
    config: &SkillsConfig,
    readonly_sources: Vec<super::SkillDirectorySource>,
) -> Result<Vec<SkillSource>> {
    let mut sources = vec![SkillSource {
        root: super::project_skills_dir(workspace_root, config)?,
        kind: SkillSourceKind::Project,
        priority: 0,
    }];
    for (index, source) in readonly_sources.into_iter().enumerate() {
        let priority = u16::try_from(index + 1).map_err(|_| {
            PureError::ConfigError("too many local read-only Skill sources to rank".to_string())
        })?;
        sources.push(SkillSource {
            root: source.root,
            kind: source.source,
            priority,
        });
    }
    Ok(sources)
}

fn resolve_local_readonly_skill_sources_with_agents_user_dir(
    config: &SkillsConfig,
    system_dir: Option<&Path>,
    agents_user_dir: Option<&Path>,
) -> Result<Vec<super::SkillDirectorySource>> {
    let mut sources = Vec::new();
    let configured_user_dir = super::resolve_user_skills_dir(config)?;
    sources.push(super::SkillDirectorySource::new(
        configured_user_dir.clone(),
        SkillSourceKind::User,
    ));
    if let Some(root) = agents_user_dir
        && root != configured_user_dir
    {
        sources.push(super::SkillDirectorySource::new(
            root,
            SkillSourceKind::User,
        ));
    }
    if config.system.enabled
        && let Some(root) = system_dir
    {
        sources.push(super::SkillDirectorySource::new(
            root,
            SkillSourceKind::System,
        ));
    }
    for external_dir in &config.external_dirs {
        sources.push(super::SkillDirectorySource::new(
            super::provider::external_source_root(external_dir)?,
            SkillSourceKind::External,
        ));
    }
    Ok(sources)
}

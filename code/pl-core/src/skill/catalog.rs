use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use pl_protocol::Result;

use super::scanning::{find_skill_files, metadata_from_file};
use super::util::platform_matches;
use super::{
    SkillCatalog, SkillMetadata, SkillSelectionRequest, SkillSelector, SkillSource, SkillSourceKind,
};
use crate::config::SkillsConfig;

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
        let agents_user_dir = super::util::agents_user_skills_dir().ok();
        Self::discover_with_agents_user_dir(
            workspace_root,
            config,
            system_dir,
            agents_user_dir.as_deref(),
        )
    }

    pub(super) fn discover_with_agents_user_dir(
        workspace_root: &Path,
        config: &SkillsConfig,
        system_dir: Option<&Path>,
        agents_user_dir: Option<&Path>,
    ) -> Result<Self> {
        let project_dir = super::project_skills_dir(workspace_root, config)?;
        let mut warnings = Vec::new();
        let mut by_name: BTreeMap<String, (SkillMetadata, u8)> = BTreeMap::new();
        let disabled = config
            .disabled
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();

        let sources = skill_sources(workspace_root, config, system_dir, agents_user_dir)?;
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
        return "# Skills\n当前项目未发现可用 skills。完成可复用流程后，可用 `skill_manage` 写入项目 `skills/` 目录。".to_string();
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

pub(crate) fn build_skill_suggestions_from_catalog(
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
    agents_user_dir: Option<&Path>,
) -> Result<Vec<SkillSource>> {
    let mut sources = Vec::new();
    sources.push(SkillSource {
        root: super::project_skills_dir(workspace_root, config)?,
        kind: SkillSourceKind::Project,
        priority: 0,
    });
    let configured_user_dir = super::resolve_user_skills_dir(config)?;
    sources.push(SkillSource {
        root: configured_user_dir.clone(),
        kind: SkillSourceKind::User,
        priority: 1,
    });
    if let Some(root) = agents_user_dir
        && root != configured_user_dir
    {
        sources.push(SkillSource {
            root: root.to_path_buf(),
            kind: SkillSourceKind::User,
            priority: 2,
        });
    }
    if config.system.enabled
        && let Some(root) = system_dir
    {
        sources.push(SkillSource {
            root: root.to_path_buf(),
            kind: SkillSourceKind::System,
            priority: 3,
        });
    }
    for external_dir in &config.external_dirs {
        sources.push(SkillSource {
            root: super::provider::external_source_root(external_dir)?,
            kind: SkillSourceKind::External,
            priority: 4,
        });
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::super::{SKILL_FILE_NAME, SkillsConfig, bump_project_view};
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

    fn discover_without_agents_home(
        workspace: &Path,
        config: &SkillsConfig,
        system: Option<&Path>,
    ) -> SkillCatalog {
        SkillCatalog::discover_with_agents_user_dir(workspace, config, system, None).unwrap()
    }

    fn catalog_skill(name: &str, description: &str) -> SkillMetadata {
        let path = PathBuf::from("skills").join(name);
        SkillMetadata {
            name: name.to_string(),
            description: description.to_string(),
            category: None,
            platforms: Vec::new(),
            source: SkillSourceKind::Project,
            path: path.clone(),
            provider_id: super::super::SkillProviderId::new("test").unwrap(),
            invocation: super::super::SkillInvocationPolicy::default(),
            resource_base: super::super::SkillResourceBase::Directory { path },
        }
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
        config.system.enabled = false;
        config
            .external_dirs
            .push(external.to_string_lossy().to_string());

        let catalog = discover_without_agents_home(&workspace, &config, None);

        assert_eq!(catalog.skills.len(), 1);
        assert_eq!(catalog.skills[0].description, "project");
        assert_eq!(catalog.skills[0].source, SkillSourceKind::Project);
        let _ = fs::remove_dir_all(workspace);
        let _ = fs::remove_dir_all(user);
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn disabled_skills_are_filtered() {
        let workspace = temp_dir("disabled");
        write_skill(&workspace.join("skills").join("hidden"), "hidden", "hidden");
        let mut config = SkillsConfig {
            disabled: vec!["hidden".to_string()],
            ..SkillsConfig::default()
        };
        config.system.enabled = false;

        let catalog = discover_without_agents_home(&workspace, &config, None);

        assert!(catalog.skills.is_empty());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn usage_update_replaces_existing_file_atomically() {
        let project = temp_dir("usage-replace");
        let skill_dir = project.join("skills").join("usage");
        write_skill(&skill_dir, "usage", "usage");
        let skill = SkillMetadata {
            name: "usage".to_string(),
            description: "usage".to_string(),
            category: None,
            platforms: Vec::new(),
            source: SkillSourceKind::Project,
            path: skill_dir.clone(),
            provider_id: super::super::SkillProviderId::new("local-filesystem").unwrap(),
            invocation: super::super::SkillInvocationPolicy::default(),
            resource_base: super::super::SkillResourceBase::Directory {
                path: skill_dir.clone(),
            },
        };

        bump_project_view(&project, &skill).unwrap();
        bump_project_view(&project, &skill).unwrap();

        let usage: super::super::SkillUsage = serde_json::from_str(
            &fs::read_to_string(skill_dir.join(super::super::USAGE_FILE_NAME)).unwrap(),
        )
        .unwrap();
        assert_eq!(usage.views, 2);
        assert_eq!(usage.uses, 2);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn corrupted_usage_is_observable() {
        let project = temp_dir("usage-corrupted");
        let skill_dir = project.join("skills").join("usage");
        write_skill(&skill_dir, "usage", "usage");
        fs::write(skill_dir.join(super::super::USAGE_FILE_NAME), "not-json").unwrap();
        let skill = SkillMetadata {
            name: "usage".to_string(),
            description: "usage".to_string(),
            category: None,
            platforms: Vec::new(),
            source: SkillSourceKind::Project,
            path: skill_dir.clone(),
            provider_id: super::super::SkillProviderId::new("local-filesystem").unwrap(),
            invocation: super::super::SkillInvocationPolicy::default(),
            resource_base: super::super::SkillResourceBase::Directory { path: skill_dir },
        };

        let error = bump_project_view(&project, &skill).unwrap_err().to_string();

        assert!(error.contains("failed to parse skill usage"));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn agents_user_directory_is_discovered_between_configured_user_and_system() {
        let workspace = temp_dir("agents-user-workspace");
        let configured_user = temp_dir("agents-configured-user");
        let home = temp_dir("agents-home");
        let agents_user = home.join(".agents").join("skills");
        let system = temp_dir("agents-system");
        write_skill(&configured_user.join("shared"), "shared", "configured user");
        write_skill(&agents_user.join("shared"), "shared", "agents user");
        write_skill(
            &agents_user.join("agents-only"),
            "agents-only",
            "agents user",
        );
        write_skill(&system.join("agents-only"), "agents-only", "system");
        let config = SkillsConfig {
            user_dir: configured_user.to_string_lossy().into_owned(),
            ..SkillsConfig::default()
        };

        let catalog = SkillCatalog::discover_with_agents_user_dir(
            &workspace,
            &config,
            Some(&system),
            Some(&agents_user),
        )
        .unwrap();

        assert_eq!(
            catalog.find("shared").unwrap().description,
            "configured user"
        );
        let agents_only = catalog.find("agents-only").unwrap();
        assert_eq!(agents_only.description, "agents user");
        assert_eq!(agents_only.source, SkillSourceKind::User);
        assert_eq!(agents_only.path, agents_user.join("agents-only"));
        let _ = fs::remove_dir_all(workspace);
        fs::remove_dir_all(configured_user).unwrap();
        fs::remove_dir_all(home).unwrap();
        fs::remove_dir_all(system).unwrap();
    }

    #[test]
    fn configured_and_agents_user_same_directory_is_scanned_once() {
        let workspace = temp_dir("agents-dedup-workspace");
        let agents_user = temp_dir("agents-dedup-user");
        let invalid = agents_user.join("invalid");
        fs::create_dir_all(&invalid).unwrap();
        fs::write(invalid.join(SKILL_FILE_NAME), "missing frontmatter").unwrap();
        let config = SkillsConfig {
            user_dir: agents_user.to_string_lossy().into_owned(),
            ..SkillsConfig::default()
        };

        let catalog = SkillCatalog::discover_with_agents_user_dir(
            &workspace,
            &config,
            None,
            Some(&agents_user),
        )
        .unwrap();

        assert_eq!(catalog.warnings.len(), 1);
        let _ = fs::remove_dir_all(workspace);
        fs::remove_dir_all(agents_user).unwrap();
    }

    #[test]
    fn discovers_system_skills_between_user_and_external_priority() {
        let workspace = temp_dir("system-priority-workspace");
        let user = temp_dir("system-priority-user");
        let system = temp_dir("system-priority-system");
        let external = temp_dir("system-priority-external");
        write_skill(&user.join("shared"), "shared", "user");
        write_skill(&system.join("skill-creator"), "skill-creator", "system");
        write_skill(&external.join("skill-creator"), "skill-creator", "external");
        let mut config = SkillsConfig {
            user_dir: user.to_string_lossy().to_string(),
            ..SkillsConfig::default()
        };
        config
            .external_dirs
            .push(external.to_string_lossy().to_string());
        let catalog = discover_without_agents_home(&workspace, &config, Some(&system));

        let shared = catalog.find("shared").unwrap();
        let creator = catalog.find("skill-creator").unwrap();
        assert_eq!(shared.source, SkillSourceKind::User);
        assert_eq!(creator.source, SkillSourceKind::System);
        let _ = fs::remove_dir_all(workspace);
        let _ = fs::remove_dir_all(user);
        fs::remove_dir_all(system).unwrap();
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn project_skill_shadows_system_skill() {
        let workspace = temp_dir("system-shadow-workspace");
        let user = temp_dir("system-shadow-user");
        let system = temp_dir("system-shadow-system");
        write_skill(
            &workspace.join("skills").join("skill-creator"),
            "skill-creator",
            "project override",
        );
        let config = SkillsConfig {
            user_dir: user.to_string_lossy().to_string(),
            ..SkillsConfig::default()
        };
        write_skill(&system.join("skill-creator"), "skill-creator", "system");

        let catalog = discover_without_agents_home(&workspace, &config, Some(&system));

        let creator = catalog.find("skill-creator").unwrap();
        assert_eq!(creator.source, SkillSourceKind::Project);
        assert_eq!(creator.description, "project override");
        let _ = fs::remove_dir_all(workspace);
        let _ = fs::remove_dir_all(user);
        fs::remove_dir_all(system).unwrap();
    }

    #[test]
    fn system_directory_is_not_derived_from_user_directory() {
        let workspace = temp_dir("system-independent-workspace");
        let user = temp_dir("system-independent-user");
        let explicit_system = temp_dir("system-independent-explicit");
        write_skill(
            &user.join(".system").join("legacy"),
            "legacy",
            "legacy system",
        );
        write_skill(
            &explicit_system.join("current"),
            "current",
            "current system",
        );
        let config = SkillsConfig {
            user_dir: user.to_string_lossy().to_string(),
            ..SkillsConfig::default()
        };

        let without_system = discover_without_agents_home(&workspace, &config, None);
        let with_system = discover_without_agents_home(&workspace, &config, Some(&explicit_system));

        assert!(without_system.find("legacy").is_none());
        assert!(without_system.find("current").is_none());
        assert!(with_system.find("legacy").is_none());
        assert_eq!(
            with_system.find("current").unwrap().source,
            SkillSourceKind::System
        );
        let _ = fs::remove_dir_all(workspace);
        fs::remove_dir_all(user).unwrap();
        fs::remove_dir_all(explicit_system).unwrap();
    }

    #[test]
    fn system_can_be_disabled() {
        let workspace = temp_dir("system-disabled-workspace");
        let user = temp_dir("system-disabled-user");
        let system = temp_dir("system-disabled-system");
        let mut config = SkillsConfig {
            user_dir: user.to_string_lossy().to_string(),
            ..SkillsConfig::default()
        };
        config.system.enabled = false;
        write_skill(&system.join("skill-creator"), "skill-creator", "system");

        let catalog = discover_without_agents_home(&workspace, &config, Some(&system));

        assert!(catalog.find("skill-creator").is_none());
        let _ = fs::remove_dir_all(workspace);
        let _ = fs::remove_dir_all(user);
        fs::remove_dir_all(system).unwrap();
    }

    #[test]
    fn disabled_filters_system_skill_by_name() {
        let workspace = temp_dir("system-disabled-name-workspace");
        let user = temp_dir("system-disabled-name-user");
        let system = temp_dir("system-disabled-name-system");
        let config = SkillsConfig {
            user_dir: user.to_string_lossy().to_string(),
            disabled: vec!["skill-creator".to_string()],
            ..SkillsConfig::default()
        };
        write_skill(&system.join("skill-creator"), "skill-creator", "system");
        write_skill(
            &system.join("subagent-workflow"),
            "subagent-workflow",
            "system",
        );

        let catalog = discover_without_agents_home(&workspace, &config, Some(&system));

        assert!(catalog.find("skill-creator").is_none());
        assert!(catalog.find("subagent-workflow").is_some());
        let _ = fs::remove_dir_all(workspace);
        let _ = fs::remove_dir_all(user);
        fs::remove_dir_all(system).unwrap();
    }

    #[test]
    fn skills_prompt_includes_system_readonly_guidance() {
        let workspace = temp_dir("system-prompt-workspace");
        let user = temp_dir("system-prompt-user");
        let system = temp_dir("system-prompt-system");
        let config = SkillsConfig {
            user_dir: user.to_string_lossy().to_string(),
            ..SkillsConfig::default()
        };
        write_skill(&system.join("skill-creator"), "skill-creator", "system");

        let prompt = build_skills_prompt(&workspace, &config, Some(&system))
            .unwrap()
            .unwrap();

        assert!(prompt.contains("skill-creator"));
        assert!(prompt.contains("System/User/External skills 是只读来源"));
        let _ = fs::remove_dir_all(workspace);
        let _ = fs::remove_dir_all(user);
        fs::remove_dir_all(system).unwrap();
    }

    #[test]
    fn skills_prompt_sorts_and_only_normalizes_the_model_projection() {
        let long_description = format!("first\n\tsecond   {}", "界".repeat(510));
        let mut hidden = catalog_skill("hidden", "not model visible");
        hidden.invocation.model_invocable = false;
        let catalog = SkillCatalog {
            project_dir: PathBuf::from("skills"),
            skills: vec![
                catalog_skill("Zulu", "last"),
                catalog_skill("alpha", &long_description),
                hidden,
            ],
            warnings: Vec::new(),
            complete: true,
        };

        let prompt = build_skills_prompt_from_catalog(&catalog);

        assert!(prompt.find("`alpha`").unwrap() < prompt.find("`Zulu`").unwrap());
        assert!(!prompt.contains("`hidden`"));
        assert!(prompt.contains("first second"));
        let projected = prompt
            .lines()
            .find(|line| line.starts_with("- `alpha`:"))
            .unwrap();
        assert_eq!(
            projected
                .strip_prefix("- `alpha`: ")
                .unwrap()
                .chars()
                .count(),
            500
        );
        assert!(projected.ends_with("..."));
        assert_eq!(catalog.find("alpha").unwrap().description, long_description);
    }
}

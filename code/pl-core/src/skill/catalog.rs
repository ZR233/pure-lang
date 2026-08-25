use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use pl_protocol::Result;

use super::scanning::{find_skill_files, metadata_from_file};
use super::util::{expand_home, platform_matches};
use super::{SkillCandidate, SkillCatalog, SkillMetadata, SkillSource, SkillSourceKind};
use crate::config::SkillsConfig;

impl SkillCatalog {
    /// Discovers the effective Skills catalog for one workspace.
    ///
    /// Studio passes its product-owned system directory explicitly. Product-neutral
    /// callers pass `None` and never infer a system directory from `user_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured source path cannot be resolved or the
    /// project Skills directory escapes the workspace.
    pub fn discover(
        workspace_root: &Path,
        config: &SkillsConfig,
        system_dir: Option<&Path>,
    ) -> Result<Self> {
        let project_dir = super::project_skills_dir(workspace_root, config)?;
        let mut warnings = Vec::new();
        let mut by_name: BTreeMap<String, SkillCandidate> = BTreeMap::new();
        let disabled = config
            .disabled
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();

        let sources = skill_sources(workspace_root, config, system_dir)?;
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
    if catalog.skills.is_empty() {
        return "# Skills\n当前项目未发现可用 skills。完成可复用流程后，可用 `skill_manage` 写入项目 `skills/` 目录。".to_string();
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
        "\nSystem/User/External skills 是只读来源。完成复杂任务、修复非平凡问题或发现可复用项目流程后，优先用 `skill_manage` 修补已有项目 skill；没有合适 skill 时创建新的项目 skill。不要记录一次性任务、瞬时环境失败或纯用户私密偏好。",
    );
    if !catalog.warnings.is_empty() {
        prompt.push_str("\n\n发现部分 skill 失败：\n");
        for warning in catalog.warnings.iter().take(5) {
            prompt.push_str(&format!("- {warning}\n"));
        }
    }
    prompt
}

fn skill_sources(
    workspace_root: &Path,
    config: &SkillsConfig,
    system_dir: Option<&Path>,
) -> Result<Vec<SkillSource>> {
    let mut sources = Vec::new();
    sources.push(SkillSource {
        root: super::project_skills_dir(workspace_root, config)?,
        kind: SkillSourceKind::Project,
        priority: 0,
    });
    sources.push(SkillSource {
        root: super::resolve_user_skills_dir(config)?,
        kind: SkillSourceKind::User,
        priority: 1,
    });
    if config.system.enabled
        && let Some(root) = system_dir
    {
        sources.push(SkillSource {
            root: root.to_path_buf(),
            kind: SkillSourceKind::System,
            priority: 2,
        });
    }
    for external_dir in &config.external_dirs {
        sources.push(SkillSource {
            root: expand_home(external_dir)?,
            kind: SkillSourceKind::External,
            priority: 3,
        });
    }
    Ok(sources)
}

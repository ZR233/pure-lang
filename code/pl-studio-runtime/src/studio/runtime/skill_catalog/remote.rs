use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use pl_core::config::SkillsConfig;
use pl_core::remote::RemoteWorkspaceFileBackend;
use pl_core::skill::{
    FileSystemSkillProvider, SkillDirectorySource, SkillProvider, SkillProviderRegistration,
    SkillRegistry, resolve_local_readonly_skill_sources,
};

/// 远端 workspace 本地只读目录集合的稳定 Provider ID。
pub(super) const REMOTE_LOCAL_PROVIDER_ID: &str = "remote-local-skills";

/// 组合远端 workspace 与本地只读目录的 Turn 与 Settings 共用 Skill registry。
///
/// 远端 provider 贡献 Project 源技能；本地 user/Agents/system/external 目录以
/// 只读来源并行注册。返回的 guards 必须存活到 `SkillRegistry::discover` 返回之后，
/// 否则 provider 会在发现完成前被注销。
pub(super) fn remote_workspace_registry(
    config: &SkillsConfig,
    system_skills_dir: Option<&Path>,
    remote_backend: Arc<RemoteWorkspaceFileBackend>,
) -> Result<(SkillRegistry, Vec<Arc<SkillProviderRegistration>>)> {
    let remote_provider = Arc::new(pl_core::remote::RemoteSkillProvider::new(remote_backend)?);
    let local_sources = remote_local_sources(config, system_skills_dir)?;
    register_remote_skill_providers(remote_provider, local_sources)
}

fn register_remote_skill_providers(
    remote_provider: Arc<dyn SkillProvider>,
    local_sources: Vec<SkillDirectorySource>,
) -> Result<(SkillRegistry, Vec<Arc<SkillProviderRegistration>>)> {
    let registry = SkillRegistry::new();
    let mut registrations = vec![Arc::new(registry.register(remote_provider)?)];
    if !local_sources.is_empty() {
        let provider =
            FileSystemSkillProvider::from_directories(REMOTE_LOCAL_PROVIDER_ID, local_sources)?;
        registrations.push(Arc::new(registry.register(Arc::new(provider))?));
    }
    Ok((registry, registrations))
}

/// 本地只读目录使用与本地 workspace 相同的顺序参与远端 workspace 发现。
///
/// system 目录由调用方传入，Studio 运行时总是提供物化后的预置技能目录。
pub(super) fn remote_local_sources(
    config: &SkillsConfig,
    system_skills_dir: Option<&Path>,
) -> Result<Vec<SkillDirectorySource>> {
    Ok(resolve_local_readonly_skill_sources(
        config,
        system_skills_dir,
    )?)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pl_core::skill::{SkillProviderRequest, SkillSourceKind};
    use tokio_util::sync::CancellationToken;

    use super::*;

    fn write_skill(root: &Path, name: &str, description: &str) {
        let directory = root.join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"),
        )
        .unwrap();
    }

    #[test]
    fn local_sources_match_the_core_readonly_source_contract() {
        let user_dir = std::env::temp_dir().join("pure-remote-skills-user");
        let external_dir = std::env::temp_dir().join("pure-remote-skills-external");
        let config = SkillsConfig {
            user_dir: user_dir.to_string_lossy().into_owned(),
            external_dirs: vec![external_dir.to_string_lossy().into_owned()],
            ..SkillsConfig::default()
        };
        let system_dir = Path::new("/studio/skills/.system");

        let sources = remote_local_sources(&config, Some(system_dir)).unwrap();
        let expected = resolve_local_readonly_skill_sources(&config, Some(system_dir)).unwrap();

        assert_eq!(sources, expected);
        assert_eq!(sources.first().unwrap().root, user_dir);
        assert_eq!(sources.last().unwrap().root, external_dir);
    }

    #[test]
    fn local_sources_omit_system_when_no_system_directory_is_registered() {
        let user_dir = std::env::temp_dir().join("pure-remote-skills-user-only");
        let config = SkillsConfig {
            user_dir: user_dir.to_string_lossy().into_owned(),
            ..SkillsConfig::default()
        };
        let sources = remote_local_sources(&config, None).unwrap();

        assert_eq!(
            sources,
            resolve_local_readonly_skill_sources(&config, None).unwrap()
        );
        assert!(
            sources
                .iter()
                .all(|source| { source.source != pl_core::skill::SkillSourceKind::System })
        );
    }

    #[tokio::test]
    async fn registry_registers_agents_user_source_and_keeps_remote_project_priority() {
        let root = tempfile::tempdir().unwrap();
        let remote_project = root.path().join("remote-project");
        let configured_user = root.path().join("configured-user");
        let agents_user = root.path().join("home/.agents/skills");
        write_skill(&remote_project, "shared", "remote project");
        write_skill(&configured_user, "shared", "configured user");
        write_skill(&agents_user, "shared", "agents user");
        write_skill(&agents_user, "agents-only", "agents user");

        let remote_provider = Arc::new(
            FileSystemSkillProvider::from_directories(
                "remote-project-test",
                vec![SkillDirectorySource::new(
                    &remote_project,
                    SkillSourceKind::Project,
                )],
            )
            .unwrap(),
        );
        let local_sources = vec![
            SkillDirectorySource::new(&configured_user, SkillSourceKind::User),
            SkillDirectorySource::new(&agents_user, SkillSourceKind::User),
        ];
        let (registry, registrations) =
            register_remote_skill_providers(remote_provider, local_sources).unwrap();

        let catalog = registry
            .discover(SkillProviderRequest {
                workspace_root: root.path().to_path_buf(),
                config: SkillsConfig::default(),
                system_dir: None,
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();

        assert_eq!(registrations.len(), 2);
        let shared = catalog.find("shared").unwrap();
        assert_eq!(shared.description, "remote project");
        assert_eq!(shared.source, SkillSourceKind::Project);
        let agents_only = catalog.find("agents-only").unwrap();
        assert_eq!(agents_only.description, "agents user");
        assert_eq!(agents_only.source, SkillSourceKind::User);
        assert_eq!(agents_only.path, agents_user.join("agents-only"));
    }
}

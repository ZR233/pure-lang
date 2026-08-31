use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use pl_core::config::SkillsConfig;
use pl_core::remote::RemoteWorkspaceFileBackend;
use pl_core::skill::{
    BUILTIN_MODE_PROVIDER_ID, FileSystemSkillProvider, SkillDirectorySource,
    SkillProviderRegistration, SkillRegistry, SkillSourceKind,
};

/// 远端 workspace 本地只读目录集合的稳定 Provider ID。
pub(super) const REMOTE_LOCAL_PROVIDER_ID: &str = "remote-local-skills";

/// 组合远端 workspace 与本地只读目录的 Turn 与 Settings 共用 Skill registry。
///
/// 远端 provider 贡献 Project 源技能；本地 user/system 目录与内置 Mode Skill 以
/// 只读来源并行注册。返回的 guards 必须存活到 `SkillRegistry::discover` 返回之后，
/// 否则 provider 会在发现完成前被注销。
pub(super) fn remote_workspace_registry(
    config: &SkillsConfig,
    system_skills_dir: Option<&Path>,
    remote_backend: Arc<RemoteWorkspaceFileBackend>,
) -> Result<(SkillRegistry, Vec<Arc<SkillProviderRegistration>>)> {
    let registry = SkillRegistry::new();
    let mut registrations = Vec::new();
    let remote_provider = pl_core::remote::RemoteSkillProvider::new(remote_backend)?;
    registrations.push(Arc::new(registry.register(Arc::new(remote_provider))?));
    let local_sources = remote_local_sources(config, system_skills_dir);
    if !local_sources.is_empty() {
        let provider =
            FileSystemSkillProvider::from_directories(REMOTE_LOCAL_PROVIDER_ID, local_sources)?;
        registrations.push(Arc::new(registry.register(Arc::new(provider))?));
    }
    if let Some(system_skills_dir) = system_skills_dir {
        let provider = FileSystemSkillProvider::from_directories(
            BUILTIN_MODE_PROVIDER_ID,
            vec![SkillDirectorySource::new(
                system_skills_dir,
                SkillSourceKind::System,
            )],
        )?;
        registrations.push(Arc::new(registry.register(Arc::new(provider))?));
    }
    Ok((registry, registrations))
}

/// 本地只读目录按 user 在前、system 在后的顺序参与远端 workspace 发现。
///
/// user 目录解析失败时跳过该来源而不是让整个远端发现失败；system 目录由
/// 调用方传入，Studio 运行时总是提供物化后的预置技能目录。
pub(super) fn remote_local_sources(
    config: &SkillsConfig,
    system_skills_dir: Option<&Path>,
) -> Vec<SkillDirectorySource> {
    let mut sources = Vec::new();
    if let Ok(user_dir) = pl_core::skill::resolve_user_skills_dir(config) {
        sources.push(SkillDirectorySource::new(user_dir, SkillSourceKind::User));
    }
    if let Some(system_skills_dir) = system_skills_dir {
        sources.push(SkillDirectorySource::new(
            system_skills_dir,
            SkillSourceKind::System,
        ));
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_sources_keep_user_before_system() {
        let config = SkillsConfig::default();
        let system_dir = Path::new("/studio/skills/.system");

        let sources = remote_local_sources(&config, Some(system_dir));

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].source, SkillSourceKind::User);
        assert_eq!(sources[1].source, SkillSourceKind::System);
        assert_eq!(sources[1].root, system_dir);
    }

    #[test]
    fn local_sources_survive_missing_system_directory() {
        let sources = remote_local_sources(&SkillsConfig::default(), None);

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source, SkillSourceKind::User);
    }
}

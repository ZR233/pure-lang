use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use pl_tool::remote::RemoteWorkspaceFileBackend;
use pl_tool::skill::SkillsConfig;
use pl_tool::skill::{
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
    let remote_provider = Arc::new(pl_tool::remote::RemoteSkillProvider::new(remote_backend)?);
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

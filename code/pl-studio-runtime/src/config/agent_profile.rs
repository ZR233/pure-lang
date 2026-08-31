use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use pl_protocol::{AgentProfileSnapshot, AgentWorkspaceMode};
use serde::{Deserialize, Serialize};

use crate::{PureError, Result};

use super::{ConfigPaths, ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig, StudioRole};

const SYSTEM_PROFILE_REVISION: &str = "studio-system-agent-v2";
const SYSTEM_PROFILE_IDS: [&str; 5] = [
    "explorer",
    "planner",
    "executor",
    "worktree_executor",
    "reviewer",
];

/// 单个用户 Agent TOML 的 canonical 格式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAgentProfile {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub display_name: String,
    pub description: String,
    pub when_to_use: String,
    pub system_instructions: String,
    pub provider: ProviderId,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub workspace_mode: AgentWorkspaceMode,
}

/// 单文件诊断只排除对应 Profile，不阻断其余系统或用户 Profile。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

/// 一次目录扫描得到的启用、可执行 Profile 及逐文件诊断。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentProfileCatalog {
    pub profiles: Vec<AgentProfileSnapshot>,
    pub diagnostics: Vec<AgentProfileDiagnostic>,
}

impl AgentProfileCatalog {
    pub fn discover(paths: &ConfigPaths, config: &StudioConfig) -> Self {
        Self::discover_with_disabled(paths, config, false)
    }

    /// 设置页保留被禁用的系统与用户 Profile；执行目录仍只返回启用项。
    pub fn discover_for_settings(paths: &ConfigPaths, config: &StudioConfig) -> Self {
        Self::discover_with_disabled(paths, config, true)
    }

    fn discover_with_disabled(
        paths: &ConfigPaths,
        config: &StudioConfig,
        include_disabled: bool,
    ) -> Self {
        let mut catalog = Self {
            profiles: system_profiles(config, include_disabled),
            diagnostics: Vec::new(),
        };
        let agents_dir = paths.agents_dir();
        let entries = match fs::read_dir(&agents_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return catalog,
            Err(error) => {
                catalog.diagnostics.push(AgentProfileDiagnostic {
                    path: agents_dir,
                    message: format!("failed to read Agent Profile directory: {error}"),
                });
                return catalog;
            }
        };
        let mut paths = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            match load_user_profile(&path, config, include_disabled) {
                Ok(Some(profile)) => catalog.profiles.push(profile),
                Ok(None) => {}
                Err(error) => catalog.diagnostics.push(AgentProfileDiagnostic {
                    path,
                    message: error.to_string(),
                }),
            }
        }
        catalog
    }
}

/// 原子创建或保存单个用户 Agent 文件；系统 Profile ID 永远不可写。
pub fn save_user_agent_profile(
    paths: &ConfigPaths,
    profile_id: &str,
    profile: &UserAgentProfile,
    config: &StudioConfig,
) -> Result<PathBuf> {
    validate_profile_id(profile_id)?;
    if is_system_profile_id(profile_id) {
        return Err(PureError::ConfigError(format!(
            "system Agent Profile `{profile_id}` is immutable"
        )));
    }
    validate_user_profile(profile_id, profile, config)?;
    let content = toml::to_string_pretty(profile).map_err(|error| {
        PureError::ConfigError(format!(
            "failed to serialize Agent Profile `{profile_id}`: {error}"
        ))
    })?;
    let directory = paths.agents_dir();
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{profile_id}.toml"));
    pl_core::atomic_file::write_file_atomically(&path, content.as_bytes())?;
    Ok(path)
}

pub fn is_system_profile_id(profile_id: &str) -> bool {
    SYSTEM_PROFILE_IDS.contains(&profile_id)
}

fn system_profiles(config: &StudioConfig, include_disabled: bool) -> Vec<AgentProfileSnapshot> {
    StudioRole::all()
        .into_iter()
        .filter(|role| {
            include_disabled || !config.disabled_system_agents.contains(role.key())
        })
        .filter_map(|role| match system_profile(role, config) {
            Ok(profile) => Some(profile),
            Err(error) => {
                tracing::warn!(profile_id = role.key(), %error, "system Agent Profile is unavailable");
                None
            }
        })
        .collect()
}

fn system_profile(role: StudioRole, config: &StudioConfig) -> Result<AgentProfileSnapshot> {
    let route = config.resolve_role(role)?;
    let (description, when_to_use, instructions) = match role {
        StudioRole::Explorer => (
            "只读探索代码、文档和现场事实。",
            "需要快速定位边界、依赖、实现入口或验证事实时。",
            include_str!("../prompts/explorer.md"),
        ),
        StudioRole::Planner => (
            "分析目标并形成可执行方案。",
            "需要独立梳理复杂方案、风险或阶段设计时。",
            include_str!("../prompts/planner.md"),
        ),
        StudioRole::Executor => (
            "实施明确、边界清楚的工程任务。",
            "已有目标和范围，需要修改与验证代码时。",
            include_str!("../prompts/executor.md"),
        ),
        StudioRole::WorktreeExecutor => (
            "在独立 Git worktree 中实施明确任务。",
            "需要物理隔离修改，并由主代理审查、整合和清理时。",
            include_str!("../prompts/worktree_executor.md"),
        ),
        StudioRole::Reviewer => (
            "检查实现、测试、错误路径和需求一致性。",
            "需要独立复核已完成工作并报告具体问题时。",
            include_str!("../prompts/reviewer.md"),
        ),
    };
    let snapshot = AgentProfileSnapshot {
        profile_id: role.key().to_string(),
        display_name: role.display_name().to_string(),
        description: description.to_string(),
        when_to_use: when_to_use.to_string(),
        system_instructions: instructions.to_string(),
        provider_id: route.provider_id.as_str().to_string(),
        model: route.model.slug,
        effort: route.effort.map(|effort| effort.as_str().to_string()),
        source: "studio-builtin".to_string(),
        revision: SYSTEM_PROFILE_REVISION.to_string(),
        content_hash: String::new(),
        system: true,
        enabled: !config.disabled_system_agents.contains(role.key()),
        workspace_mode: match role {
            StudioRole::Explorer | StudioRole::Planner | StudioRole::Reviewer => {
                AgentWorkspaceMode::Unrestricted
            }
            StudioRole::Executor => AgentWorkspaceMode::Directory,
            StudioRole::WorktreeExecutor => AgentWorkspaceMode::Worktree,
        },
    };
    Ok(with_content_hash(snapshot))
}

fn load_user_profile(
    path: &Path,
    config: &StudioConfig,
    include_disabled: bool,
) -> Result<Option<AgentProfileSnapshot>> {
    let profile_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| PureError::ConfigError("Agent Profile filename is not UTF-8".to_string()))?;
    validate_profile_id(profile_id)?;
    if is_system_profile_id(profile_id) {
        return Err(PureError::ConfigError(format!(
            "user Agent Profile cannot replace immutable system profile `{profile_id}`"
        )));
    }
    let content = fs::read_to_string(path)?;
    let profile: UserAgentProfile = toml::from_str(&content).map_err(|error| {
        PureError::ConfigError(format!(
            "failed to parse Agent Profile `{profile_id}`: {error}"
        ))
    })?;
    validate_user_profile(profile_id, &profile, config)?;
    if !profile.enabled && !include_disabled {
        return Ok(None);
    }
    let snapshot = AgentProfileSnapshot {
        profile_id: profile_id.to_string(),
        display_name: profile.display_name,
        description: profile.description,
        when_to_use: profile.when_to_use,
        system_instructions: profile.system_instructions,
        provider_id: profile.provider.as_str().to_string(),
        model: profile.model,
        effort: profile.effort.map(|effort| effort.as_str().to_string()),
        source: path.to_string_lossy().into_owned(),
        revision: pl_core::canonical_content_hash(content.as_bytes()),
        content_hash: String::new(),
        system: false,
        enabled: profile.enabled,
        workspace_mode: profile.workspace_mode,
    };
    Ok(Some(with_content_hash(snapshot)))
}

fn validate_user_profile(
    profile_id: &str,
    profile: &UserAgentProfile,
    config: &StudioConfig,
) -> Result<()> {
    for (field, value) in [
        ("display_name", profile.display_name.as_str()),
        ("description", profile.description.as_str()),
        ("when_to_use", profile.when_to_use.as_str()),
        ("system_instructions", profile.system_instructions.as_str()),
        ("model", profile.model.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "Agent Profile `{profile_id}` has empty {field}"
            )));
        }
    }
    let mut models = config.models.clone();
    let role = pl_core::AgentRoleId::new(profile_id)?;
    models.routes.insert(
        role.clone(),
        ModelRouteConfig {
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            effort: profile.effort.clone(),
        },
    );
    models.resolve(&role).map(|_| ())
}

fn validate_profile_id(profile_id: &str) -> Result<()> {
    let valid = !profile_id.is_empty()
        && profile_id.len() <= 64
        && profile_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(PureError::ConfigError(format!(
            "invalid Agent Profile id `{profile_id}`; use 1-64 lowercase ASCII letters, digits, '-' or '_'"
        )))
    }
}

fn with_content_hash(mut snapshot: AgentProfileSnapshot) -> AgentProfileSnapshot {
    let mut hashable = snapshot.clone();
    hashable.content_hash.clear();
    snapshot.content_hash = pl_core::canonical_content_hash(
        &serde_json::to_vec(&hashable).expect("Agent Profile snapshot must serialize"),
    );
    snapshot
}

const fn default_true() -> bool {
    true
}

/// 返回内置 Profile 的稳定集合，供配置校验和 UI 使用。
pub fn system_profile_ids() -> BTreeSet<String> {
    SYSTEM_PROFILE_IDS.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn invalid_user_file_does_not_hide_other_profiles() {
        let home = TempDir::new().unwrap();
        let paths = ConfigPaths::from_home(home.path());
        fs::create_dir_all(paths.agents_dir()).unwrap();
        fs::write(paths.agents_dir().join("broken.toml"), "not = [toml").unwrap();

        let catalog = AgentProfileCatalog::discover(&paths, &StudioConfig::default());

        assert_eq!(catalog.profiles.len(), 5);
        assert_eq!(catalog.diagnostics.len(), 1);
    }

    #[test]
    fn built_in_profiles_have_fixed_workspace_modes() {
        let home = TempDir::new().unwrap();
        let catalog = AgentProfileCatalog::discover_for_settings(
            &ConfigPaths::from_home(home.path()),
            &StudioConfig::default_config(),
        );
        let modes = catalog
            .profiles
            .iter()
            .map(|profile| (profile.profile_id.as_str(), profile.workspace_mode))
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(modes["explorer"], AgentWorkspaceMode::Unrestricted);
        assert_eq!(modes["planner"], AgentWorkspaceMode::Unrestricted);
        assert_eq!(modes["reviewer"], AgentWorkspaceMode::Unrestricted);
        assert_eq!(modes["executor"], AgentWorkspaceMode::Directory);
        assert_eq!(modes["worktree_executor"], AgentWorkspaceMode::Worktree);
    }

    #[test]
    fn older_user_profile_without_workspace_mode_defaults_to_directory() {
        let profile: UserAgentProfile = toml::from_str(
            r#"
enabled = true
display_name = "Legacy"
description = "Legacy profile"
when_to_use = "Legacy tasks"
system_instructions = "Do the task."
provider = "openai"
model = "gpt-5"
"#,
        )
        .unwrap();

        assert_eq!(profile.workspace_mode, AgentWorkspaceMode::Directory);
    }

    #[test]
    fn system_profiles_are_immutable_and_only_disabled_from_main_config() {
        let home = TempDir::new().unwrap();
        let paths = ConfigPaths::from_home(home.path());
        let config = StudioConfig::default();
        let route = config.resolve_role(StudioRole::Explorer).unwrap();
        let profile = UserAgentProfile {
            enabled: true,
            display_name: "替换".to_string(),
            description: "替换".to_string(),
            when_to_use: "替换".to_string(),
            system_instructions: "替换".to_string(),
            provider: route.provider_id,
            model: route.model.slug,
            effort: route.effort,
            workspace_mode: AgentWorkspaceMode::Directory,
        };

        let error = save_user_agent_profile(&paths, "explorer", &profile, &config).unwrap_err();
        assert!(error.to_string().contains("immutable"));

        let mut disabled = config;
        disabled
            .disabled_system_agents
            .insert("explorer".to_string());
        let catalog = AgentProfileCatalog::discover(&paths, &disabled);
        assert!(
            !catalog
                .profiles
                .iter()
                .any(|item| item.profile_id == "explorer")
        );
        let settings_catalog = AgentProfileCatalog::discover_for_settings(&paths, &disabled);
        assert!(
            settings_catalog
                .profiles
                .iter()
                .any(|item| item.profile_id == "explorer" && item.system && !item.enabled)
        );
    }

    #[test]
    fn user_profile_round_trips_as_one_atomic_toml_file() {
        let home = TempDir::new().unwrap();
        let paths = ConfigPaths::from_home(home.path());
        let config = StudioConfig::default();
        let route = config.resolve_role(StudioRole::Executor).unwrap();
        let profile = UserAgentProfile {
            enabled: true,
            display_name: "Rust 执行器".to_string(),
            description: "实现 Rust 任务".to_string(),
            when_to_use: "边界明确的 Rust 修改".to_string(),
            system_instructions: "完成实现并验证。".to_string(),
            provider: route.provider_id,
            model: route.model.slug,
            effort: route.effort,
            workspace_mode: AgentWorkspaceMode::Worktree,
        };

        let path = save_user_agent_profile(&paths, "rust-executor", &profile, &config).unwrap();
        assert_eq!(path, paths.agents_dir().join("rust-executor.toml"));
        let catalog = AgentProfileCatalog::discover(&paths, &config);
        assert!(catalog.diagnostics.is_empty());
        assert!(
            catalog
                .profiles
                .iter()
                .any(|item| { item.profile_id == "rust-executor" && !item.system })
        );
    }

    #[test]
    fn disabled_user_profile_is_only_visible_in_settings_catalog() {
        let home = TempDir::new().unwrap();
        let paths = ConfigPaths::from_home(home.path());
        let config = StudioConfig::default();
        let route = config.resolve_role(StudioRole::Executor).unwrap();
        let profile = UserAgentProfile {
            enabled: false,
            display_name: "暂停使用".to_string(),
            description: "保留但不执行".to_string(),
            when_to_use: "重新启用后使用".to_string(),
            system_instructions: "完成明确任务。".to_string(),
            provider: route.provider_id,
            model: route.model.slug,
            effort: route.effort,
            workspace_mode: AgentWorkspaceMode::Unrestricted,
        };
        save_user_agent_profile(&paths, "paused-agent", &profile, &config).unwrap();

        assert!(
            AgentProfileCatalog::discover(&paths, &config)
                .profiles
                .iter()
                .all(|item| item.profile_id != "paused-agent")
        );
        assert!(
            AgentProfileCatalog::discover_for_settings(&paths, &config)
                .profiles
                .iter()
                .any(|item| item.profile_id == "paused-agent" && !item.enabled)
        );
    }
}

use std::sync::{Arc, Mutex, RwLock};

use crate::studio::unix_seconds;
use crate::{PureError, Result};
use serde::{Deserialize, Serialize};

use super::{ConfigStore, StudioConfig};

/// Settings desired state 的唯一进程内 owner。
///
/// 查询只克隆内存 snapshot；磁盘读写只发生在构造、显式 reload 或 CAS update command。
#[derive(Clone)]
pub struct ConfigRuntime {
    store: ConfigStore,
    command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<ConfigRuntimeSnapshot>>,
}

/// 已校验 Studio 配置及其单调 revision。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRuntimeSnapshot {
    pub revision: u64,
    pub updated_at: i64,
    pub config: StudioConfig,
}

/// Parameters resolved from one configuration revision for a newly created product Agent.
#[derive(Clone)]
pub struct ResolvedAgentProfile {
    /// The same snapshot used for Profile and route resolution, for coherent resource assembly.
    pub config: StudioConfig,
    pub revision: u64,
    pub profile: pl_protocol::AgentProfileSnapshot,
    pub route: pl_model::config::ResolvedModelRoute,
}

impl std::fmt::Debug for ResolvedAgentProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedAgentProfile")
            .field("revision", &self.revision)
            .field("profile_id", &self.profile.profile_id)
            .finish_non_exhaustive()
    }
}

/// Stable Settings owner failures used by transport adapters.
#[derive(Debug, thiserror::Error)]
pub enum ConfigRuntimeError {
    #[error("settings revision conflict: expected {expected}, actual {actual}")]
    StaleRevision { expected: u64, actual: u64 },
    #[error(transparent)]
    Core(#[from] PureError),
}

type ConfigRuntimeResult<T> = std::result::Result<T, ConfigRuntimeError>;

impl From<ConfigRuntimeError> for PureError {
    fn from(error: ConfigRuntimeError) -> Self {
        match error {
            ConfigRuntimeError::Core(error) => error,
            ConfigRuntimeError::StaleRevision { expected, actual } => PureError::ConfigError(
                format!("settings revision conflict: expected {expected}, actual {actual}"),
            ),
        }
    }
}

impl ConfigRuntime {
    /// 从磁盘加载并校验初始 desired config。
    pub fn initialize(store: ConfigStore) -> ConfigRuntimeResult<Self> {
        let config = store.load_for_startup()?;
        Ok(Self {
            store,
            command_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(ConfigRuntimeSnapshot {
                revision: 1,
                updated_at: unix_seconds(),
                config,
            })),
        })
    }

    /// 返回内存 canonical snapshot，不访问磁盘。
    pub fn read(&self) -> ConfigRuntimeResult<ConfigRuntimeSnapshot> {
        self.state
            .read()
            .map(|state| state.clone())
            .map_err(|_| config_runtime_poisoned())
    }

    /// 从独立 TOML 目录发现本次可用的 Agent Profile。
    pub fn agent_profiles(&self) -> ConfigRuntimeResult<super::AgentProfileCatalog> {
        let config = self.read()?.config;
        Ok(super::AgentProfileCatalog::discover(
            self.store.paths(),
            &config,
        ))
    }

    /// Resolves an enabled Profile and its model from the same snapshot; performs profile-file IO.
    ///
    /// # Errors
    /// Returns unknown/disabled Profile, configuration or provider route errors.
    pub fn resolve_agent_profile(
        &self,
        profile_id: &str,
    ) -> ConfigRuntimeResult<ResolvedAgentProfile> {
        if profile_id == super::StudioRole::Planner.key() {
            return Err(PureError::ConfigError(
                "planner is reserved for the main agent; this child role is retired and cannot resume".into(),
            ).into());
        }
        let snapshot = self.read()?;
        let catalog = super::AgentProfileCatalog::discover(self.store.paths(), &snapshot.config);
        let profile = catalog
            .profiles
            .into_iter()
            .find(|profile| profile.profile_id == profile_id && profile.enabled)
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "Agent Profile is missing or disabled: {profile_id}"
                ))
            })?;
        let route = super::resolve_profile_route(&snapshot.config, &profile)?;
        Ok(ResolvedAgentProfile {
            config: snapshot.config,
            revision: snapshot.revision,
            profile,
            route,
        })
    }

    /// 返回设置页使用的 Profile；其中包含被禁用的内置 Profile。
    pub fn agent_profiles_for_settings(&self) -> ConfigRuntimeResult<super::AgentProfileCatalog> {
        let config = self.read()?.config;
        Ok(super::AgentProfileCatalog::discover_for_settings(
            self.store.paths(),
            &config,
        ))
    }

    /// 原子创建或替换一个用户 Agent Profile 文件。
    pub fn save_user_agent_profile(
        &self,
        expected_revision: u64,
        profile_id: &str,
        profile: &super::UserAgentProfile,
    ) -> ConfigRuntimeResult<ConfigRuntimeSnapshot> {
        let _command = self
            .command_lock
            .lock()
            .map_err(|_| config_runtime_poisoned())?;
        let current = self.read()?;
        ensure_revision(expected_revision, current.revision)?;
        let mut state = self.state.write().map_err(|_| config_runtime_poisoned())?;
        super::save_user_agent_profile(self.store.paths(), profile_id, profile, &current.config)?;
        let next = ConfigRuntimeSnapshot {
            revision: current.revision.saturating_add(1),
            updated_at: unix_seconds(),
            config: current.config,
        };
        *state = next.clone();
        Ok(next)
    }

    /// 使用 expected revision 原子保存完整 desired config。
    pub fn replace(
        &self,
        expected_revision: u64,
        config: StudioConfig,
    ) -> ConfigRuntimeResult<ConfigRuntimeSnapshot> {
        self.update(expected_revision, |_| Ok(config))
    }

    /// 在串行 command 边界内变换、校验、持久化并发布配置。
    pub fn update(
        &self,
        expected_revision: u64,
        edit: impl FnOnce(&StudioConfig) -> Result<StudioConfig>,
    ) -> ConfigRuntimeResult<ConfigRuntimeSnapshot> {
        let _command = self
            .command_lock
            .lock()
            .map_err(|_| config_runtime_poisoned())?;
        let current = self.read()?;
        ensure_revision(expected_revision, current.revision)?;
        let next_config = edit(&current.config)?;
        next_config.validate()?;

        // 文件和 credential IO 不持有 state lock；command lock 只负责串行化 Settings 命令。
        self.store.save(&next_config)?;

        let next = ConfigRuntimeSnapshot {
            revision: current.revision.saturating_add(1),
            updated_at: unix_seconds(),
            config: next_config,
        };
        *self.state.write().map_err(|_| config_runtime_poisoned())? = next.clone();
        Ok(next)
    }

    /// 显式从磁盘重新加载配置；普通 read 永不调用此方法。
    pub fn reload_from_disk(
        &self,
        expected_revision: u64,
    ) -> ConfigRuntimeResult<ConfigRuntimeSnapshot> {
        let _command = self
            .command_lock
            .lock()
            .map_err(|_| config_runtime_poisoned())?;
        let current = self.read()?;
        ensure_revision(expected_revision, current.revision)?;
        let config = self.store.load_or_default()?;
        let next = ConfigRuntimeSnapshot {
            revision: current.revision.saturating_add(1),
            updated_at: unix_seconds(),
            config,
        };
        *self.state.write().map_err(|_| config_runtime_poisoned())? = next.clone();
        Ok(next)
    }
}

fn ensure_revision(expected: u64, actual: u64) -> ConfigRuntimeResult<()> {
    if expected != actual {
        return Err(ConfigRuntimeError::StaleRevision { expected, actual });
    }
    Ok(())
}

fn config_runtime_poisoned() -> ConfigRuntimeError {
    PureError::ConfigError("ConfigRuntime state lock is poisoned".to_string()).into()
}

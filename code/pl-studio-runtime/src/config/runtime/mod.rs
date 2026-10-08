mod catalog;

use std::sync::{Arc, Mutex, RwLock};

use crate::studio::unix_seconds;
use crate::{PureError, Result};
use serde::{Deserialize, Serialize};

use super::model_catalog::{self, Observation};
use super::{AgentProfilesResource, AgentProfilesSnapshot, ConfigStore, StudioConfig};
use pl_model::config::ProviderId;
use pl_protocol::studio::StudioModelCatalogStatus;
use std::collections::BTreeMap;

/// Settings desired state 的唯一进程内 owner。
///
/// 查询只克隆内存 snapshot；磁盘读写只发生在构造、显式 reload 或 CAS update command。
#[derive(Clone)]
pub struct ConfigRuntime {
    store: ConfigStore,
    command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<RuntimeState>>,
    catalog_updates: tokio::sync::broadcast::Sender<CatalogChange>,
    /// 配置 owner 持有的 canonical Agent Profiles 资源；命令路径扫描，查询只读缓存。
    profiles: AgentProfilesResource,
}

struct RuntimeState {
    desired: StudioConfig,
    snapshot: ConfigRuntimeSnapshot,
    observations: BTreeMap<ProviderId, Observation>,
    closing: bool,
}

#[derive(Clone)]
pub(crate) struct CatalogChange {
    pub snapshot: ConfigRuntimeSnapshot,
    pub affected: Vec<ProviderId>,
}

/// 已校验 Studio 配置及其单调 revision。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRuntimeSnapshot {
    pub revision: u64,
    pub model_catalog_revision: u64,
    pub model_catalogs: BTreeMap<ProviderId, StudioModelCatalogStatus>,
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
        let desired = store.load_for_startup()?;
        let observations = model_catalog::reconcile(store.paths(), &desired, &BTreeMap::new())?;
        let config = model_catalog::effective(&desired, &observations)?;
        config.validate_declarations()?;
        super::AgentProfileCatalog::validate_for_startup(store.paths(), &config)?;
        // canonical Agent Profiles 资源：启动卸载一次扫描，之后查询/订阅只读缓存。
        let profiles = AgentProfilesResource::new(store.paths(), &config);
        let (catalog_updates, _) = tokio::sync::broadcast::channel(64);
        Ok(Self {
            store,
            command_lock: Arc::new(Mutex::new(())),
            catalog_updates,
            state: Arc::new(RwLock::new(RuntimeState {
                desired,
                closing: false,
                snapshot: ConfigRuntimeSnapshot {
                    revision: 1,
                    model_catalog_revision: 1,
                    model_catalogs: statuses(&observations),
                    updated_at: unix_seconds(),
                    config,
                },
                observations,
            })),
            profiles,
        })
    }

    /// 返回内存 canonical snapshot，不访问磁盘。
    pub fn read(&self) -> ConfigRuntimeResult<ConfigRuntimeSnapshot> {
        self.state
            .read()
            .map(|state| state.snapshot.clone())
            .map_err(|_| config_runtime_poisoned())
    }

    /// 从已发布缓存取得启用 Profile 的执行视图；不扫描文件。
    pub fn agent_profiles(&self) -> ConfigRuntimeResult<super::AgentProfileCatalog> {
        Ok(self.profiles.read().catalog.enabled_only())
    }

    /// 读取已发布的 Agent Profiles 资源快照（完整配置与诊断）；纯缓存读取。
    pub fn agent_profiles_snapshot(&self) -> ConfigRuntimeResult<AgentProfilesSnapshot> {
        Ok(self.profiles.read())
    }

    /// 订阅 Agent Profiles 资源变更唤醒；值为最新 revision。
    pub fn subscribe_agent_profiles(&self) -> tokio::sync::watch::Receiver<u64> {
        self.profiles.subscribe()
    }

    /// Resolves an enabled Profile and its model from the same snapshot; reads the published cache.
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
        let catalog = self.profiles.read().catalog.enabled_only();
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
            ..current
        };
        state.snapshot = next.clone();
        drop(state);
        // 配置命令采用一次新的目录扫描；资源未变化时不提升 revision、不通知观察者。
        // scan 不持有 state 写锁（与 `update` / `reload_from_disk` 一致）。
        self.profiles.adopt_scan(self.store.paths(), &next.config);
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
        let (desired, observations) = {
            let state = self.state.read().map_err(|_| config_runtime_poisoned())?;
            if state.closing {
                return Err(PureError::ConfigError("settings owner is closing".into()).into());
            }
            (state.desired.clone(), state.observations.clone())
        };
        let mut next_config = edit(&desired)?;
        for provider in next_config.models.providers.values_mut() {
            provider.clear_model_catalog_overlay();
        }
        let next_observations =
            model_catalog::reconcile(self.store.paths(), &next_config, &observations)?;
        let effective = model_catalog::effective(&next_config, &next_observations)?;
        effective.validate_declarations()?;
        validate_changed_selections(&desired, &effective)?;

        // 文件和 credential IO 不持有 state lock；command lock 只负责串行化 Settings 命令。
        self.store.save(&next_config)?;

        let next = ConfigRuntimeSnapshot {
            revision: current.revision.saturating_add(1),
            model_catalog_revision: current.model_catalog_revision.saturating_add(u64::from(
                statuses(&next_observations) != current.model_catalogs
                    || effective.models.providers != current.config.models.providers,
            )),
            model_catalogs: statuses(&next_observations),
            updated_at: unix_seconds(),
            config: effective,
        };
        invalidate_observations(&observations, &next_observations);
        let mut state = self.state.write().map_err(|_| config_runtime_poisoned())?;
        state.desired = next_config;
        state.observations = next_observations;
        state.snapshot = next.clone();
        drop(state);
        // 配置命令采用一次新的目录扫描；资源未变化时不提升 revision、不通知观察者。
        self.profiles.adopt_scan(self.store.paths(), &next.config);
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
        let desired = self.store.load_or_default()?;
        let observations = self
            .state
            .read()
            .map_err(|_| config_runtime_poisoned())?
            .observations
            .clone();
        let next_observations =
            model_catalog::reconcile(self.store.paths(), &desired, &observations)?;
        let config = model_catalog::effective(&desired, &next_observations)?;
        config.validate_declarations()?;
        super::AgentProfileCatalog::validate_for_startup(self.store.paths(), &config)?;
        let next = ConfigRuntimeSnapshot {
            revision: current.revision.saturating_add(1),
            model_catalog_revision: current.model_catalog_revision.saturating_add(1),
            model_catalogs: statuses(&next_observations),
            updated_at: unix_seconds(),
            config,
        };
        invalidate_observations(&observations, &next_observations);
        let mut state = self.state.write().map_err(|_| config_runtime_poisoned())?;
        state.desired = desired;
        state.observations = next_observations;
        state.snapshot = next.clone();
        drop(state);
        // 显式重扫；目录未变化时不提升 revision。
        self.profiles.adopt_scan(self.store.paths(), &next.config);
        Ok(next)
    }
}

fn statuses(
    observations: &BTreeMap<ProviderId, Observation>,
) -> BTreeMap<ProviderId, StudioModelCatalogStatus> {
    observations
        .iter()
        .map(|(id, observation)| (id.clone(), observation.status.clone()))
        .collect()
}

fn invalidate_observations(
    old: &BTreeMap<ProviderId, Observation>,
    next: &BTreeMap<ProviderId, Observation>,
) {
    for (id, observation) in old {
        if next
            .get(id)
            .is_none_or(|next| next.generation != observation.generation)
        {
            observation.cancellation.cancel();
        }
    }
}

fn validate_changed_selections(old: &StudioConfig, next: &StudioConfig) -> Result<()> {
    for (role, route) in &next.models.routes {
        if old.models.routes.get(role) != Some(route) {
            next.models.resolve_route(role.clone(), route)?;
        }
    }
    for (mode, route) in &next.mode_model_routes {
        if old.mode_model_routes.get(mode) != Some(route) {
            next.models
                .resolve_route(super::StudioRole::Planner.id(), route)?;
        }
    }
    for (id, provider) in &next.models.providers {
        let models = provider.effective_models()?;
        for (slug, mode) in provider.connection_overrides() {
            if old
                .models
                .providers
                .get(id)
                .and_then(|old| old.connection_overrides().get(slug))
                == Some(mode)
            {
                continue;
            }
            let model = models
                .iter()
                .find(|model| model.slug == *slug)
                .ok_or_else(|| {
                    PureError::ConfigError(
                        "new connection override references unavailable model".into(),
                    )
                })?;
            if !model
                .binding
                .transport
                .supported_connection_modes
                .contains(mode)
            {
                return Err(PureError::ConfigError(
                    "new connection override is unsupported".into(),
                ));
            }
        }
    }
    Ok(())
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

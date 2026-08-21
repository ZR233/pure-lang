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
        let config = store.load_or_default()?;
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

    #[cfg(test)]
    pub(crate) fn store(&self) -> &ConfigStore {
        &self.store
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigPaths;

    fn runtime(name: &str) -> ConfigRuntime {
        let home = tempfile::Builder::new()
            .prefix(&format!("config-runtime-{name}-"))
            .tempdir()
            .unwrap()
            .keep();
        ConfigRuntime::initialize(ConfigStore::new(ConfigPaths::from_home(home))).unwrap()
    }

    #[test]
    fn read_does_not_observe_external_file_changes() {
        let runtime = runtime("read-memory");
        let before = runtime.read().unwrap();
        let path = runtime.store().paths().config_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "invalid external content").unwrap();

        let after = runtime.read().unwrap();

        assert_eq!(after, before);
    }

    #[test]
    fn stale_revision_cannot_overwrite_new_config() {
        let runtime = runtime("cas");
        let initial = runtime.read().unwrap();
        let first = runtime
            .update(initial.revision, |config| {
                let mut config = config.clone();
                config.instructions.user = "new".to_string();
                Ok(config)
            })
            .unwrap();

        let error = runtime
            .update(initial.revision, |config| Ok(config.clone()))
            .unwrap_err()
            .to_string();

        assert!(error.contains("revision conflict"));
        assert_eq!(runtime.read().unwrap(), first);
    }
}

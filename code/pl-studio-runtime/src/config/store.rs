use std::collections::BTreeSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{PureError, Result};

use super::credential::{CredentialStore, MemoryCredentialStore, SystemCredentialStore};
use super::{STUDIO_CONFIG_DIR_NAME, STUDIO_CONFIG_FILE_NAME, StudioConfig, StudioRole};
use pl_protocol::ThreadModeId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    config_dir: PathBuf,
    config_file: PathBuf,
}

#[derive(Clone)]
pub struct ConfigStore {
    paths: ConfigPaths,
    credentials: Arc<dyn CredentialStore>,
}

/// Studio 启动时自动恢复不兼容配置所生成的报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRecoveryReport {
    backup_path: PathBuf,
}

impl ConfigRecoveryReport {
    /// 返回不兼容配置逐字备份的绝对路径。
    pub fn backup_path(&self) -> &Path {
        &self.backup_path
    }
}

#[derive(Debug)]
pub(crate) struct StartupConfigLoad {
    pub config: StudioConfig,
    pub recovery: Option<ConfigRecoveryReport>,
}

#[derive(Debug, Clone, Copy)]
enum ConfigIncompatibilityKind {
    Parse,
    InlineCredential,
    Validation,
}

impl ConfigIncompatibilityKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::InlineCredential => "inlineCredential",
            Self::Validation => "validation",
        }
    }
}

#[derive(Debug)]
struct IncompatibleConfig {
    kind: ConfigIncompatibilityKind,
    error: PureError,
}

impl IncompatibleConfig {
    fn new(kind: ConfigIncompatibilityKind, error: PureError) -> Self {
        Self { kind, error }
    }

    fn into_error(self) -> PureError {
        self.error
    }
}

impl std::fmt::Debug for ConfigStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigStore")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

impl ConfigPaths {
    pub fn for_current_user() -> Result<Self> {
        Ok(Self::from_home(user_home_dir()?))
    }

    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let config_dir = home.into().join(STUDIO_CONFIG_DIR_NAME);
        Self::from_config_dir(config_dir)
    }

    pub fn from_config_dir(config_dir: impl Into<PathBuf>) -> Self {
        let config_dir = config_dir.into();
        let config_file = config_dir.join(STUDIO_CONFIG_FILE_NAME);
        Self {
            config_dir,
            config_file,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    /// 用户 Agent Profile 的独立 TOML 目录。
    pub fn agents_dir(&self) -> PathBuf {
        self.config_dir.join("agents")
    }
}

impl ConfigStore {
    pub fn default_app() -> Result<Self> {
        Ok(Self::with_credential_store(
            ConfigPaths::for_current_user()?,
            Arc::new(SystemCredentialStore),
        ))
    }

    pub fn for_studio_home(studio_home: impl Into<PathBuf>) -> Self {
        Self::with_credential_store(
            ConfigPaths::from_config_dir(studio_home),
            Arc::new(SystemCredentialStore),
        )
    }

    /// 创建使用进程内凭据存储的隔离配置实例。
    ///
    /// 生产桌面应用必须使用 [`Self::default_app`]，避免测试、fixture 或 driver 修改用户的系统凭据库。
    pub fn new(paths: ConfigPaths) -> Self {
        Self::with_credential_store(paths, Arc::new(MemoryCredentialStore::default()))
    }

    fn with_credential_store(paths: ConfigPaths, credentials: Arc<dyn CredentialStore>) -> Self {
        Self { paths, credentials }
    }

    pub fn paths(&self) -> &ConfigPaths {
        &self.paths
    }

    pub fn config_exists(&self) -> bool {
        self.paths.config_file().exists()
    }

    pub fn load_or_default(&self) -> Result<StudioConfig> {
        if !self.config_exists() {
            let mut config = StudioConfig::default_config();
            self.hydrate_credentials(&mut config)?;
            return Ok(config);
        }
        let content = fs::read_to_string(self.paths.config_file())?;
        let mut config = parse_current_config(&content)?;
        self.hydrate_credentials(&mut config)?;
        Ok(config)
    }

    pub(crate) fn load_for_startup(&self) -> Result<StartupConfigLoad> {
        if !self.config_exists() {
            let mut config = StudioConfig::default_config();
            self.hydrate_credentials(&mut config)?;
            return Ok(StartupConfigLoad {
                config,
                recovery: None,
            });
        }
        let content = fs::read(self.paths.config_file())?;
        // Version dispatch precedes the legacy recovery path: a failed supported migration
        // must preserve the source config and credentials instead of resetting them.
        #[derive(serde::Deserialize)]
        struct Version {
            schema_version: u32,
        }
        if let Some(version) = std::str::from_utf8(&content)
            .ok()
            .and_then(|text| toml::from_str::<Version>(text).ok())
            .map(|version| version.schema_version)
            .filter(|version| matches!(version, 18 | 19))
        {
            return self.migrate_legacy_config_for_startup(&content, version);
        }
        match parse_startup_config(&content) {
            Ok(mut config) => {
                self.hydrate_credentials(&mut config)?;
                Ok(StartupConfigLoad {
                    config,
                    recovery: None,
                })
            }
            Err(incompatible) => self.replace_incompatible_for_startup(&content, incompatible),
        }
    }

    fn migrate_legacy_config_for_startup(
        &self,
        original: &[u8],
        source_version: u32,
    ) -> Result<StartupConfigLoad> {
        self.migrate_legacy_config_for_startup_with(original, source_version, |path, content| {
            pl_tool::workspace::write_file_atomically(path, content).map_err(Into::into)
        })
    }

    fn migrate_legacy_config_for_startup_with(
        &self,
        original: &[u8],
        source_version: u32,
        replace: impl FnOnce(&Path, &[u8]) -> Result<()>,
    ) -> Result<StartupConfigLoad> {
        let text = std::str::from_utf8(original).map_err(|error| {
            PureError::ConfigError(format!("invalid schema {source_version} UTF-8: {error}"))
        })?;
        let mut config = parse_typed_config(text).map_err(IncompatibleConfig::into_error)?;
        reject_inline_credentials(&config).map_err(IncompatibleConfig::into_error)?;
        if source_version == 18 {
            config.disabled_system_agents.remove("planner");
        }
        let planner_route = config
            .models
            .routes
            .remove(&StudioRole::Planner.id())
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "schema {source_version} config is missing planner model route"
                ))
            })?;
        config.mode_model_routes = std::collections::BTreeMap::from([
            (ThreadModeId::simple(), planner_route.clone()),
            (ThreadModeId::task(), planner_route),
        ]);
        config.schema_version = super::STUDIO_CONFIG_SCHEMA_VERSION;
        config.validate()?;
        let persisted = serialize_persisted_config(&config)?;
        // Provider identities do not change. Read credentials before committing; never rewrite them.
        self.hydrate_credentials(&mut config)?;
        let backup = write_config_backup(self.paths.config_file(), original, "migrated")?;
        replace(self.paths.config_file(), persisted.as_bytes())?;
        tracing::info!(
            backup_path = %backup.display(),
            source_version,
            target_version = super::STUDIO_CONFIG_SCHEMA_VERSION,
            "migrated Studio config"
        );
        Ok(StartupConfigLoad {
            config,
            recovery: None,
        })
    }

    pub fn load(&self) -> Result<StudioConfig> {
        let content = fs::read_to_string(self.paths.config_file())?;
        let mut config = parse_current_config(&content)?;
        self.hydrate_credentials(&mut config)?;
        Ok(config)
    }

    pub fn save(&self, config: &StudioConfig) -> Result<()> {
        config.validate()?;
        let persisted_provider_ids = self.persisted_provider_ids()?;
        let content = serialize_persisted_config(config)?;
        fs::create_dir_all(self.paths.config_dir())?;
        let previous = self.apply_credentials(config, persisted_provider_ids)?;
        if let Err(error) =
            pl_tool::workspace::write_file_atomically(self.paths.config_file(), content.as_bytes())
        {
            self.restore_credentials(&previous);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn init_default(&self) -> Result<StudioConfig> {
        if self.config_exists() {
            return Err(PureError::ConfigError(format!(
                "config already exists: {}",
                self.paths.config_file().display()
            )));
        }
        let config = StudioConfig::default_config();
        self.save(&config)?;
        Ok(config)
    }

    fn hydrate_credentials(&self, config: &mut StudioConfig) -> Result<()> {
        for (provider_id, provider) in &mut config.models.providers {
            provider.bearer_token = self.credentials.load(provider_id.as_str())?;
        }
        Ok(())
    }

    fn replace_incompatible_for_startup(
        &self,
        original: &[u8],
        incompatible: IncompatibleConfig,
    ) -> Result<StartupConfigLoad> {
        self.replace_incompatible_for_startup_with(
            original,
            incompatible,
            |config_path, persisted| {
                pl_tool::workspace::write_file_atomically(config_path, persisted)
                    .map_err(Into::into)
            },
        )
    }

    fn replace_incompatible_for_startup_with(
        &self,
        original: &[u8],
        incompatible: IncompatibleConfig,
        replace: impl FnOnce(&Path, &[u8]) -> Result<()>,
    ) -> Result<StartupConfigLoad> {
        let mut config = StudioConfig::default_config();
        config.validate()?;
        let persisted = serialize_persisted_config(&config)?;
        self.hydrate_credentials(&mut config)?;
        let backup_path = write_rejected_backup(self.paths.config_file(), original)?;
        replace(self.paths.config_file(), persisted.as_bytes())?;
        tracing::warn!(
            config_path = %self.paths.config_file().display(),
            backup_path = %backup_path.display(),
            incompatibility = incompatible.kind.as_str(),
            "replaced incompatible Studio config with current defaults"
        );
        Ok(StartupConfigLoad {
            config,
            recovery: Some(ConfigRecoveryReport { backup_path }),
        })
    }

    fn persisted_provider_ids(&self) -> Result<BTreeSet<String>> {
        if !self.config_exists() {
            return Ok(BTreeSet::new());
        }
        let content = fs::read_to_string(self.paths.config_file())?;
        let persisted = parse_current_config(&content)?;
        Ok(persisted
            .models
            .providers
            .keys()
            .map(|provider_id| provider_id.as_str().to_string())
            .collect())
    }

    fn apply_credentials(
        &self,
        config: &StudioConfig,
        mut provider_ids: BTreeSet<String>,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut previous = Vec::new();
        provider_ids.extend(
            config
                .models
                .providers
                .keys()
                .map(|provider_id| provider_id.as_str().to_string()),
        );
        for provider_id in provider_ids {
            let desired = config
                .models
                .providers
                .iter()
                .find(|(candidate, _)| candidate.as_str() == provider_id)
                .and_then(|(_, provider)| provider.bearer_token.as_deref())
                .filter(|secret| !secret.trim().is_empty());
            let old = self.credentials.load(&provider_id)?;
            previous.push((provider_id.clone(), old));
            let result = match desired {
                Some(secret) => self.credentials.save(&provider_id, secret),
                None => self.credentials.delete(&provider_id),
            }
            .and_then(|()| {
                let actual = self.credentials.load(&provider_id)?;
                if actual.as_deref() == desired {
                    Ok(())
                } else {
                    Err(PureError::ConfigError(format!(
                        "system credential verification failed for provider {provider_id}"
                    )))
                }
            });
            if let Err(error) = result {
                self.restore_credentials(&previous);
                return Err(error);
            }
        }
        Ok(previous)
    }

    fn restore_credentials(&self, previous: &[(String, Option<String>)]) {
        for (provider_id, secret) in previous.iter().rev() {
            let result = match secret {
                Some(secret) => self.credentials.save(provider_id, secret),
                None => self.credentials.delete(provider_id),
            };
            if let Err(error) = result {
                tracing::error!(%error, provider_id, "回滚系统凭据失败");
            }
        }
    }
}

fn parse_current_config(content: &str) -> Result<StudioConfig> {
    parse_config(content).map_err(IncompatibleConfig::into_error)
}

fn parse_startup_config(content: &[u8]) -> std::result::Result<StudioConfig, IncompatibleConfig> {
    let content = std::str::from_utf8(content).map_err(|error| {
        IncompatibleConfig::new(
            ConfigIncompatibilityKind::Parse,
            PureError::ConfigError(format!("failed to parse Studio config as UTF-8: {error}")),
        )
    })?;
    parse_config(content)
}

fn parse_config(content: &str) -> std::result::Result<StudioConfig, IncompatibleConfig> {
    let config = parse_typed_config(content)?;
    reject_inline_credentials(&config)?;
    config
        .validate()
        .map_err(|error| IncompatibleConfig::new(ConfigIncompatibilityKind::Validation, error))?;
    Ok(config)
}

fn parse_typed_config(content: &str) -> std::result::Result<StudioConfig, IncompatibleConfig> {
    toml::from_str(content).map_err(|error| {
        IncompatibleConfig::new(
            ConfigIncompatibilityKind::Parse,
            PureError::ConfigError(format!("failed to parse Studio config: {error}")),
        )
    })
}

fn reject_inline_credentials(config: &StudioConfig) -> std::result::Result<(), IncompatibleConfig> {
    if config
        .models
        .providers
        .values()
        .any(|provider| provider.bearer_token.is_some())
    {
        return Err(IncompatibleConfig::new(
            ConfigIncompatibilityKind::InlineCredential,
            PureError::ConfigError(
                "current schema forbids inline provider bearer_token; use the Studio credential store"
                    .to_string(),
            ),
        ));
    }
    Ok(())
}

fn write_rejected_backup(config_path: &Path, content: &[u8]) -> Result<PathBuf> {
    write_config_backup(config_path, content, "rejected")
}

fn write_config_backup(config_path: &Path, content: &[u8], kind: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    write_config_backup_at(config_path, content, kind, stamp)
}

#[cfg(test)]
fn write_rejected_backup_at(config_path: &Path, content: &[u8], stamp: u128) -> Result<PathBuf> {
    write_config_backup_at(config_path, content, "rejected", stamp)
}

fn write_config_backup_at(
    config_path: &Path,
    content: &[u8],
    kind: &str,
    stamp: u128,
) -> Result<PathBuf> {
    for collision in 0..u32::MAX {
        let backup_path = config_backup_path(config_path, kind, stamp, collision);
        let mut backup = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup_path)
        {
            Ok(backup) => backup,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let result = backup.write_all(content).and_then(|()| backup.sync_all());
        if let Err(error) = result {
            drop(backup);
            let _ = fs::remove_file(&backup_path);
            return Err(error.into());
        }
        return Ok(backup_path);
    }
    Err(PureError::ConfigError(
        "could not allocate a unique Studio config backup path".to_string(),
    ))
}

#[cfg(test)]
fn rejected_backup_path(config_path: &Path, stamp: u128, collision: u32) -> PathBuf {
    config_backup_path(config_path, "rejected", stamp, collision)
}

fn config_backup_path(config_path: &Path, kind: &str, stamp: u128, collision: u32) -> PathBuf {
    let file_name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(super::STUDIO_CONFIG_FILE_NAME);
    let suffix = if collision == 0 {
        format!("{kind}.{stamp}.bak")
    } else {
        format!("{kind}.{stamp}.{collision}.bak")
    };
    config_path.with_file_name(format!("{file_name}.{suffix}"))
}

fn serialize_persisted_config(config: &StudioConfig) -> Result<String> {
    let mut persisted = config.clone();
    clear_inline_credentials(&mut persisted);
    toml::to_string_pretty(&persisted).map_err(|error| {
        PureError::ConfigError(format!("failed to serialize Studio config: {error}"))
    })
}

fn clear_inline_credentials(config: &mut StudioConfig) {
    for provider in config.models.providers.values_mut() {
        provider.bearer_token = None;
    }
}

fn user_home_dir() -> Result<PathBuf> {
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];

    HOME_VARS
        .iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| PureError::ConfigError("could not resolve user home directory".to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::config::{ReasoningEffort, STUDIO_CONFIG_SCHEMA_VERSION};

    fn temp_home(name: &str) -> PathBuf {
        tempfile::Builder::new()
            .prefix(&format!("pl-studio-config-{name}-"))
            .tempdir()
            .unwrap()
            .keep()
    }

    fn test_store(name: &str) -> ConfigStore {
        ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home(name)),
            Arc::new(MemoryCredentialStore::default()),
        )
    }

    fn assert_recovered_with_backup(
        store: &ConfigStore,
        startup: &StartupConfigLoad,
        original: &[u8],
    ) {
        assert_eq!(startup.config, StudioConfig::default_config());
        let report = startup.recovery.as_ref().unwrap();
        assert_eq!(
            report.backup_path().parent(),
            Some(store.paths().config_dir())
        );
        assert!(
            report
                .backup_path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("config.toml.rejected.")
        );
        assert_eq!(fs::read(report.backup_path()).unwrap(), original);
        assert_eq!(store.load().unwrap(), StudioConfig::default_config());
    }

    fn rejected_backups(store: &ConfigStore) -> Vec<PathBuf> {
        fs::read_dir(store.paths().config_dir())
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("config.toml.rejected."))
            })
            .collect()
    }

    fn legacy_config(schema_version: u32) -> String {
        let mut config = StudioConfig::default_config();
        config.schema_version = schema_version;
        config
            .models
            .routes
            .remove(&super::super::StudioRole::WorktreeExecutor.id());
        toml::to_string_pretty(&config).unwrap()
    }

    fn main_agent_config(schema_version: u32) -> StudioConfig {
        let mut config = StudioConfig::default_config();
        config.schema_version = schema_version;
        let planner_route = config
            .mode_model_routes
            .remove(&ThreadModeId::simple())
            .unwrap();
        config.mode_model_routes.clear();
        config
            .models
            .routes
            .insert(StudioRole::Planner.id(), planner_route);
        config
    }

    #[test]
    fn schema_18_and_19_migration_preserves_routes_credentials_and_other_settings() {
        for source_version in [18, 19] {
            let store = test_store(&format!("main-agent-migration-{source_version}"));
            let mut old = main_agent_config(source_version);
            old.ui.follow_active_turn = false;
            old.models
                .routes
                .get_mut(&StudioRole::Planner.id())
                .unwrap()
                .effort = Some(ReasoningEffort::new("max"));
            old.models.providers.values_mut().next().unwrap().name = "My provider".into();
            old.instructions.developer = "Keep my preferences".into();
            if source_version == 18 {
                old.disabled_system_agents = ["planner".into(), "explorer".into()].into();
            }
            let provider_id = old
                .models
                .providers
                .keys()
                .next()
                .unwrap()
                .as_str()
                .to_string();
            store
                .credentials
                .save(&provider_id, "migration-test-secret")
                .unwrap();
            let original = toml::to_string_pretty(&old).unwrap();
            fs::create_dir_all(store.paths().config_dir()).unwrap();
            fs::write(store.paths().config_file(), &original).unwrap();
            let mut expected = old;
            expected.schema_version = STUDIO_CONFIG_SCHEMA_VERSION;
            expected.disabled_system_agents.remove("planner");
            let planner_route = expected
                .models
                .routes
                .remove(&StudioRole::Planner.id())
                .unwrap();
            expected.mode_model_routes = BTreeMap::from([
                (ThreadModeId::simple(), planner_route.clone()),
                (ThreadModeId::task(), planner_route),
            ]);
            store.hydrate_credentials(&mut expected).unwrap();
            let loaded = store.load_for_startup().unwrap();
            assert!(loaded.recovery.is_none());
            assert_eq!(loaded.config, expected);
            assert_eq!(store.load().unwrap(), expected);
            let backups: Vec<_> = fs::read_dir(store.paths().config_dir())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.to_string_lossy().contains(".migrated."))
                .collect();
            assert_eq!(backups.len(), 1);
            assert_eq!(fs::read_to_string(&backups[0]).unwrap(), original);
            let persisted = fs::read(store.paths().config_file()).unwrap();
            assert_eq!(store.load_for_startup().unwrap().config, expected);
            assert_eq!(fs::read(store.paths().config_file()).unwrap(), persisted);
            assert_eq!(
                store.credentials.load(&provider_id).unwrap().as_deref(),
                Some("migration-test-secret")
            );
        }
    }

    #[test]
    fn schema_18_migration_failures_preserve_original_and_can_retry() {
        let store = test_store("main-agent-migration-failure");
        let mut old = main_agent_config(18);
        old.disabled_system_agents.insert("planner".into());
        old.disabled_system_agents.insert("unknown".into());
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let invalid = toml::to_string_pretty(&old).unwrap();
        fs::write(store.paths().config_file(), &invalid).unwrap();
        assert!(store.load_for_startup().is_err());
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            invalid
        );
        assert!(rejected_backups(&store).is_empty());

        old.disabled_system_agents.remove("unknown");
        let original = toml::to_string_pretty(&old).unwrap();
        fs::write(store.paths().config_file(), &original).unwrap();
        let error = store
            .migrate_legacy_config_for_startup_with(original.as_bytes(), 18, |_, _| {
                Err(std::io::Error::other("injected atomic replace failure").into())
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected atomic replace failure")
        );
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            original
        );
        assert!(store.load().is_err()); // Explicit reload never migrates.
        assert!(store.load_for_startup().unwrap().recovery.is_none());
        assert!(
            !store
                .load()
                .unwrap()
                .disabled_system_agents
                .contains("planner")
        );
    }

    #[test]
    fn obsolete_theme_preference_does_not_reset_or_change_other_settings() {
        for preference in [None, Some(true), Some(false)] {
            let store = test_store("obsolete-theme");
            let mut expected = StudioConfig::default_config();
            expected.ui.follow_active_turn = false;
            expected.ui.compact_timeline = true;
            let mut document = toml::Value::try_from(&expected).unwrap();
            let ui = document.get_mut("ui").unwrap().as_table_mut().unwrap();
            ui.remove("follow_system_theme");
            if let Some(preference) = preference {
                ui.insert(
                    "follow_system_theme".into(),
                    toml::Value::Boolean(preference),
                );
            }
            let original = toml::to_string_pretty(&document).unwrap();
            fs::create_dir_all(store.paths().config_dir()).unwrap();
            fs::write(store.paths().config_file(), &original).unwrap();
            let startup = store.load_for_startup().unwrap();
            assert!(startup.recovery.is_none());
            assert_eq!(
                fs::read_to_string(store.paths().config_file()).unwrap(),
                original
            );
            assert_eq!(startup.config, expected);
            store.save(&startup.config).unwrap();
            let persisted: toml::Value =
                toml::from_str(&fs::read_to_string(store.paths().config_file()).unwrap()).unwrap();
            assert!(persisted["ui"].get("follow_system_theme").is_none());
            assert_eq!(store.load().unwrap(), expected);
        }
    }

    #[test]
    fn save_persists_no_secret_and_load_hydrates_from_credential_store() {
        let store = test_store("roundtrip");
        let mut config = StudioConfig::default_config();
        config
            .models
            .providers
            .values_mut()
            .next()
            .unwrap()
            .bearer_token = Some("system-secret".to_string());

        store.save(&config).unwrap();

        let persisted = fs::read_to_string(store.paths().config_file()).unwrap();
        assert!(!persisted.contains("system-secret"));
        assert!(!persisted.contains("bearer_token ="));
        assert_eq!(store.load().unwrap(), config);
    }

    #[test]
    fn old_schemas_are_backed_up_and_replaced_before_current_settings_reload() {
        for schema_version in [15, 16, 17] {
            let store = test_store(&format!("schema-{schema_version}"));
            fs::create_dir_all(store.paths().config_dir()).unwrap();
            let legacy = legacy_config(schema_version);
            fs::write(store.paths().config_file(), &legacy).unwrap();
            let startup = store.load_for_startup().unwrap();
            assert_eq!(startup.config, StudioConfig::default_config());
            let backup = startup.recovery.unwrap().backup_path;
            assert_eq!(fs::read(backup).unwrap(), legacy.as_bytes());
            assert_eq!(store.load().unwrap(), startup.config);
        }
    }

    #[test]
    fn future_schema_is_backed_up_and_replaced_during_startup() {
        let store = test_store("future-schema");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let future = toml::to_string_pretty(&StudioConfig::default_config())
            .unwrap()
            .replace("schema_version = 20", "schema_version = 4294967295");
        fs::write(store.paths().config_file(), &future).unwrap();

        let startup = store.load_for_startup().unwrap();

        assert_recovered_with_backup(&store, &startup, future.as_bytes());
    }

    #[test]
    fn malformed_config_is_backed_up_and_replaced_during_startup() {
        let store = test_store("malformed");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), "not-toml").unwrap();

        let startup = store.load_for_startup().unwrap();

        assert_recovered_with_backup(&store, &startup, b"not-toml");
    }

    #[test]
    fn inline_bearer_token_is_backed_up_without_accessing_legacy_credentials() {
        let credentials = Arc::new(RecordingCredentialStore::default());
        let store = ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home("inline-secret")),
            credentials.clone(),
        );
        let mut config = config_with_provider_secret("legacy-provider", "forbidden-secret");
        config
            .models
            .providers
            .values_mut()
            .next()
            .unwrap()
            .bearer_token = Some("forbidden-secret".to_string());
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(
            store.paths().config_file(),
            toml::to_string_pretty(&config).unwrap(),
        )
        .unwrap();

        let original = fs::read(store.paths().config_file()).unwrap();
        let startup = store.load_for_startup().unwrap();
        let persisted = fs::read_to_string(store.paths().config_file()).unwrap();

        assert_eq!(startup.config, StudioConfig::default_config());
        assert_eq!(
            fs::read(startup.recovery.unwrap().backup_path()).unwrap(),
            original
        );
        assert!(!persisted.contains("forbidden-secret"));
        assert!(!persisted.contains("bearer_token ="));
        assert_eq!(
            credentials.loads.lock().unwrap().as_slice(),
            ["deepseek".to_string()]
        );
        assert!(credentials.saves.lock().unwrap().is_empty());
        assert!(credentials.deletes.lock().unwrap().is_empty());
    }

    #[test]
    fn invalid_current_schema_config_is_backed_up_and_replaced_during_startup() {
        let store = test_store("invalid-current-schema");
        let mut invalid = StudioConfig::default_config();
        invalid.models.providers.clear();
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let invalid_toml = toml::to_string_pretty(&invalid).unwrap();
        fs::write(store.paths().config_file(), &invalid_toml).unwrap();

        let startup = store.load_for_startup().unwrap();

        assert_recovered_with_backup(&store, &startup, invalid_toml.as_bytes());
    }

    #[test]
    fn config_read_failure_does_not_replace_existing_path() {
        let store = test_store("config-read-failure");
        fs::create_dir_all(store.paths().config_file()).unwrap();

        let error = store.load_for_startup().unwrap_err();

        assert!(!error.to_string().is_empty());
        assert!(store.paths().config_file().is_dir());
    }

    #[test]
    fn default_credential_failure_preserves_incompatible_config() {
        let credentials = Arc::new(ReadbackFailingCredentialStore::default());
        let store = ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home("replacement-credential-failure")),
            credentials.clone(),
        );
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), "not-toml").unwrap();
        credentials.reads_until_failure.lock().unwrap().replace(0);

        let error = store.load_for_startup().unwrap_err().to_string();

        assert!(error.contains("readback failure"));
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            "not-toml"
        );
        assert!(rejected_backups(&store).is_empty());
    }

    #[test]
    fn backup_writer_uses_a_non_overwriting_collision_suffix() {
        let home = temp_home("backup-collision");
        let config_path = home.join("config.toml");
        let first = rejected_backup_path(&config_path, 42, 0);
        fs::write(&first, "existing").unwrap();

        let backup = write_rejected_backup_at(&config_path, b"original", 42).unwrap();

        assert_eq!(backup, rejected_backup_path(&config_path, 42, 1));
        assert_eq!(fs::read_to_string(first).unwrap(), "existing");
        assert_eq!(fs::read(backup).unwrap(), b"original");
    }

    #[test]
    fn backup_writer_failure_does_not_create_a_partial_backup() {
        let home = temp_home("backup-failure");
        let missing = home.join("missing").join("config.toml");

        let error = write_rejected_backup_at(&missing, b"original", 42).unwrap_err();

        assert!(!error.to_string().is_empty());
        assert!(!rejected_backup_path(&missing, 42, 0).exists());
    }

    #[test]
    fn atomic_replacement_failure_preserves_original_after_backup() {
        let store = test_store("replacement-failure");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let original = b"not-toml";
        fs::write(store.paths().config_file(), original).unwrap();
        let incompatible = IncompatibleConfig::new(
            ConfigIncompatibilityKind::Parse,
            PureError::ConfigError("invalid test config".to_string()),
        );

        let error = store
            .replace_incompatible_for_startup_with(
                original,
                incompatible,
                |_config_path, _persisted| {
                    Err(PureError::ConfigError(
                        "injected replacement failure".to_string(),
                    ))
                },
            )
            .unwrap_err();

        assert!(error.to_string().contains("injected replacement failure"));
        assert_eq!(fs::read(store.paths().config_file()).unwrap(), original);
        let backups = rejected_backups(&store);
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0]).unwrap(), original);
    }

    #[test]
    fn renaming_provider_moves_credential_and_deletes_old_account() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        let store = ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home("rename-provider")),
            credentials.clone(),
        );
        let config = config_with_provider_secret("deepseek", "secret");
        store.save(&config).unwrap();

        let renamed = config_with_provider_secret("renamed", "new-secret");
        store.save(&renamed).unwrap();

        assert_eq!(credentials.load("deepseek").unwrap(), None);
        assert_eq!(
            credentials.load("renamed").unwrap().as_deref(),
            Some("new-secret")
        );
    }

    #[test]
    fn credential_readback_failure_rolls_back_current_provider() {
        let credentials = Arc::new(ReadbackFailingCredentialStore::default());
        let store = ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home("credential-readback-failure")),
            credentials.clone(),
        );
        let config = config_with_provider_secret("deepseek", "new-secret");
        credentials.arm_failure();

        let error = store.save(&config).unwrap_err().to_string();

        assert!(error.contains("readback failure"));
        assert_eq!(credentials.values.lock().unwrap().get("deepseek"), None);
        assert!(!store.paths().config_file().exists());
    }

    #[test]
    fn independent_stores_do_not_share_in_memory_credentials() {
        let paths = ConfigPaths::from_home(temp_home("isolated-stores"));
        let first = ConfigStore::new(paths.clone());
        let second = ConfigStore::new(paths);
        first
            .save(&config_with_provider_secret("deepseek", "first-secret"))
            .unwrap();

        let loaded = second.load().unwrap();

        assert_eq!(
            loaded
                .models
                .providers
                .values()
                .next()
                .unwrap()
                .bearer_token,
            None
        );
    }

    #[test]
    fn save_refuses_to_overwrite_invalid_existing_config() {
        let store = test_store("invalid-existing-config");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), "not-toml").unwrap();

        let error = store
            .save(&config_with_provider_secret("deepseek", "secret"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("failed to parse Studio config"));
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            "not-toml"
        );
    }

    #[test]
    fn atomic_config_write_failure_rolls_back_credentials() {
        let paths = ConfigPaths::from_home(temp_home("atomic-write-failure"));
        let credentials = Arc::new(TargetBlockingCredentialStore {
            target: paths.config_file().to_path_buf(),
            values: Mutex::new(BTreeMap::new()),
        });
        let store = ConfigStore::with_credential_store(paths, credentials.clone());

        let error = store
            .save(&config_with_provider_secret("deepseek", "secret"))
            .unwrap_err()
            .to_string();

        assert!(!error.is_empty());
        assert_eq!(credentials.load("deepseek").unwrap(), None);
        assert!(store.paths().config_file().is_dir());
    }

    fn config_with_provider_secret(provider_id: &str, secret: &str) -> StudioConfig {
        let mut config = StudioConfig::default_config();
        let (_, mut provider) = config.models.providers.pop_first().unwrap();
        provider.bearer_token = Some(secret.to_string());
        let provider_id = super::super::ProviderId::new(provider_id).unwrap();
        for route in config.models.routes.values_mut() {
            route.provider = provider_id.clone();
        }
        for route in config.mode_model_routes.values_mut() {
            route.provider = provider_id.clone();
        }
        config.models.providers.insert(provider_id, provider);
        config
    }

    #[derive(Default)]
    struct RecordingCredentialStore {
        loads: Mutex<Vec<String>>,
        saves: Mutex<Vec<String>>,
        deletes: Mutex<Vec<String>>,
    }

    impl CredentialStore for RecordingCredentialStore {
        fn load(&self, provider_id: &str) -> Result<Option<String>> {
            self.loads.lock().unwrap().push(provider_id.to_string());
            Ok(None)
        }

        fn save(&self, provider_id: &str, _secret: &str) -> Result<()> {
            self.saves.lock().unwrap().push(provider_id.to_string());
            Ok(())
        }

        fn delete(&self, provider_id: &str) -> Result<()> {
            self.deletes.lock().unwrap().push(provider_id.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct ReadbackFailingCredentialStore {
        values: Mutex<BTreeMap<String, String>>,
        reads_until_failure: Mutex<Option<usize>>,
    }

    impl ReadbackFailingCredentialStore {
        fn arm_failure(&self) {
            self.reads_until_failure.lock().unwrap().replace(1);
        }
    }

    impl CredentialStore for ReadbackFailingCredentialStore {
        fn load(&self, provider_id: &str) -> Result<Option<String>> {
            let mut reads_until_failure = self.reads_until_failure.lock().unwrap();
            if let Some(remaining) = reads_until_failure.as_mut() {
                if *remaining == 0 {
                    *reads_until_failure = None;
                    return Err(PureError::ConfigError("readback failure".to_string()));
                }
                *remaining -= 1;
            }
            Ok(self.values.lock().unwrap().get(provider_id).cloned())
        }

        fn save(&self, provider_id: &str, secret: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap()
                .insert(provider_id.to_string(), secret.to_string());
            Ok(())
        }

        fn delete(&self, provider_id: &str) -> Result<()> {
            self.values.lock().unwrap().remove(provider_id);
            Ok(())
        }
    }

    struct TargetBlockingCredentialStore {
        target: PathBuf,
        values: Mutex<BTreeMap<String, String>>,
    }

    impl CredentialStore for TargetBlockingCredentialStore {
        fn load(&self, provider_id: &str) -> Result<Option<String>> {
            Ok(self.values.lock().unwrap().get(provider_id).cloned())
        }

        fn save(&self, provider_id: &str, secret: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap()
                .insert(provider_id.to_string(), secret.to_string());
            fs::create_dir_all(&self.target)?;
            Ok(())
        }

        fn delete(&self, provider_id: &str) -> Result<()> {
            self.values.lock().unwrap().remove(provider_id);
            Ok(())
        }
    }
}

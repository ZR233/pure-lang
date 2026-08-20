use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{PureError, Result};

use super::credential::{CredentialStore, MemoryCredentialStore, SystemCredentialStore};
use super::{STUDIO_CONFIG_DIR_NAME, STUDIO_CONFIG_FILE_NAME, StudioConfig};

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

#[derive(Debug, Clone, Copy)]
enum ConfigIncompatibility {
    Parse,
    InlineCredential,
    Validation,
}

impl ConfigIncompatibility {
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
    kind: ConfigIncompatibility,
    error: PureError,
}

impl IncompatibleConfig {
    fn new(kind: ConfigIncompatibility, error: PureError) -> Self {
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
        let mut config = match parse_current_config(&content) {
            Ok(config) => config,
            Err(incompatible) => {
                return self.replace_incompatible_with_default(incompatible.kind);
            }
        };
        self.hydrate_credentials(&mut config)?;
        Ok(config)
    }

    pub fn load(&self) -> Result<StudioConfig> {
        let content = fs::read_to_string(self.paths.config_file())?;
        let mut config = parse_current_config(&content).map_err(IncompatibleConfig::into_error)?;
        self.hydrate_credentials(&mut config)?;
        Ok(config)
    }

    pub fn save(&self, config: &StudioConfig) -> Result<()> {
        config.validate()?;
        let persisted_provider_ids = self.persisted_provider_ids()?;
        let content = serialize_persisted_config(config)?;
        fs::create_dir_all(self.paths.config_dir())?;
        let previous = self.apply_credentials(config, persisted_provider_ids)?;
        if let Err(error) = pl_core::atomic_file::write_file_atomically(
            self.paths.config_file(),
            content.as_bytes(),
        ) {
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

    fn replace_incompatible_with_default(
        &self,
        incompatibility: ConfigIncompatibility,
    ) -> Result<StudioConfig> {
        let mut config = StudioConfig::default_config();
        config.validate()?;
        self.hydrate_credentials(&mut config)?;
        let content = serialize_persisted_config(&config)?;
        fs::create_dir_all(self.paths.config_dir())?;
        pl_core::atomic_file::write_file_atomically(self.paths.config_file(), content.as_bytes())?;
        tracing::warn!(
            config_path = %self.paths.config_file().display(),
            incompatibility = incompatibility.as_str(),
            "replaced incompatible Studio config with defaults"
        );
        Ok(config)
    }

    fn hydrate_credentials(&self, config: &mut StudioConfig) -> Result<()> {
        for (provider_id, provider) in &mut config.models.providers {
            provider.bearer_token = self.credentials.load(provider_id.as_str())?;
        }
        Ok(())
    }

    fn persisted_provider_ids(&self) -> Result<BTreeSet<String>> {
        if !self.config_exists() {
            return Ok(BTreeSet::new());
        }
        let content = fs::read_to_string(self.paths.config_file())?;
        let persisted = parse_current_config(&content).map_err(IncompatibleConfig::into_error)?;
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

fn parse_current_config(content: &str) -> std::result::Result<StudioConfig, IncompatibleConfig> {
    let config: StudioConfig = toml::from_str(content).map_err(|error| {
        IncompatibleConfig::new(
            ConfigIncompatibility::Parse,
            PureError::ConfigError(format!("failed to parse Studio config: {error}")),
        )
    })?;
    if config
        .models
        .providers
        .values()
        .any(|provider| provider.bearer_token.is_some())
    {
        return Err(IncompatibleConfig::new(
            ConfigIncompatibility::InlineCredential,
            PureError::ConfigError(
                "schema 14 forbids inline provider bearer_token; use the Studio credential store"
                    .to_string(),
            ),
        ));
    }
    config
        .validate()
        .map_err(|error| IncompatibleConfig::new(ConfigIncompatibility::Validation, error))?;
    Ok(config)
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
    fn old_schema_is_replaced_with_current_defaults() {
        let store = test_store("old-schema");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let legacy = toml::to_string_pretty(&StudioConfig::default_config())
            .unwrap()
            .replace("schema_version = 14", "schema_version = 13");
        fs::write(store.paths().config_file(), legacy).unwrap();

        let loaded = store.load_or_default().unwrap();

        assert_eq!(loaded, StudioConfig::default_config());
        assert_eq!(store.load().unwrap(), StudioConfig::default_config());
    }

    #[test]
    fn malformed_config_is_replaced_with_current_defaults() {
        let store = test_store("malformed");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), "not-toml").unwrap();

        let loaded = store.load_or_default().unwrap();

        assert_eq!(loaded, StudioConfig::default_config());
        assert_eq!(store.load().unwrap(), StudioConfig::default_config());
    }

    #[test]
    fn schema_14_inline_bearer_token_is_discarded_with_incompatible_config() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        let store = ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home("inline-secret")),
            credentials.clone(),
        );
        let mut config = StudioConfig::default_config();
        let provider_id = config
            .models
            .providers
            .keys()
            .next()
            .unwrap()
            .as_str()
            .to_string();
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

        let loaded = store.load_or_default().unwrap();
        let persisted = fs::read_to_string(store.paths().config_file()).unwrap();

        assert_eq!(loaded, StudioConfig::default_config());
        assert!(!persisted.contains("forbidden-secret"));
        assert!(!persisted.contains("bearer_token ="));
        assert_eq!(credentials.load(&provider_id).unwrap(), None);
    }

    #[test]
    fn invalid_current_schema_config_is_replaced_with_defaults() {
        let store = test_store("invalid-current-schema");
        let mut invalid = StudioConfig::default_config();
        invalid.models.providers.clear();
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(
            store.paths().config_file(),
            toml::to_string_pretty(&invalid).unwrap(),
        )
        .unwrap();

        let loaded = store.load_or_default().unwrap();

        assert_eq!(loaded, StudioConfig::default_config());
        assert_eq!(store.load().unwrap(), StudioConfig::default_config());
    }

    #[test]
    fn config_read_failure_does_not_replace_existing_path() {
        let store = test_store("config-read-failure");
        fs::create_dir_all(store.paths().config_file()).unwrap();

        let error = store.load_or_default().unwrap_err();

        assert!(!error.to_string().is_empty());
        assert!(store.paths().config_file().is_dir());
    }

    #[test]
    fn credential_read_failure_preserves_incompatible_config() {
        let credentials = Arc::new(ReadbackFailingCredentialStore::default());
        let store = ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home("replacement-credential-failure")),
            credentials.clone(),
        );
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), "not-toml").unwrap();
        credentials.reads_until_failure.lock().unwrap().replace(0);

        let error = store.load_or_default().unwrap_err().to_string();

        assert!(error.contains("readback failure"));
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            "not-toml"
        );
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
        config.models.providers.insert(provider_id, provider);
        config
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

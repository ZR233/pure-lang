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

    /// 读取并校验 canonical 配置；配置文件缺失时采用内存默认配置。
    ///
    /// 已存在的文件在解析、校验或内联凭据拒绝失败时原样保留并返回错误，
    /// 不写回默认值，也不触碰系统凭据关联。
    pub fn load_or_default(&self) -> Result<StudioConfig> {
        if !self.config_exists() {
            let mut config = StudioConfig::default_config();
            self.hydrate_credentials(&mut config)?;
            return Ok(config);
        }
        self.load_validated_from_disk()
    }

    pub fn load(&self) -> Result<StudioConfig> {
        self.load_validated_from_disk()
    }

    /// 启动、显式重载与 `load` 共用的读取路径：按当前 schema 类型化解析、
    /// 校验并注入系统凭据；任何失败都保留磁盘原文件。
    fn load_validated_from_disk(&self) -> Result<StudioConfig> {
        let content = fs::read(self.paths.config_file())?;
        let mut config = parse_validated_bytes(&content)?;
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

    fn persisted_provider_ids(&self) -> Result<BTreeSet<String>> {
        if !self.config_exists() {
            return Ok(BTreeSet::new());
        }
        let content = fs::read(self.paths.config_file())?;
        let persisted = parse_validated_bytes(&content)?;
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

fn parse_validated_bytes(content: &[u8]) -> Result<StudioConfig> {
    let content = std::str::from_utf8(content).map_err(|error| {
        PureError::ConfigError(format!("failed to parse Studio config as UTF-8: {error}"))
    })?;
    parse_validated_config(content)
}

fn parse_validated_config(content: &str) -> Result<StudioConfig> {
    let config: StudioConfig = match toml::from_str(content) {
        Ok(config) => config,
        Err(error) => return Err(config_parse_error(content, &error)),
    };
    reject_inline_credentials(&config)?;
    config.validate()?;
    Ok(config)
}

/// 将类型化解析失败转换为不含文档正文与凭据的诊断。
///
/// `toml::de::Error` 的 `Display` 会打印出错源代码行，可能包含内联 token；
/// 这里只使用脱敏的 `message()`。当文档声明了内联 provider 凭据时，
/// 优先返回显式的凭据拒绝诊断。
fn config_parse_error(content: &str, error: &toml::de::Error) -> PureError {
    if document_declares_inline_credentials(content) {
        return inline_credential_error();
    }
    PureError::ConfigError(format!(
        "failed to parse Studio config: {}",
        error.message()
    ))
}

fn document_declares_inline_credentials(content: &str) -> bool {
    let Ok(value) = toml::from_str::<toml::Value>(content) else {
        return false;
    };
    value
        .get("models")
        .and_then(|models| models.get("providers"))
        .and_then(toml::Value::as_table)
        .is_some_and(|providers| {
            providers.values().any(|provider| {
                provider
                    .as_table()
                    .is_some_and(|provider| provider.contains_key("bearer_token"))
            })
        })
}

fn reject_inline_credentials(config: &StudioConfig) -> Result<()> {
    if config
        .models
        .providers
        .values()
        .any(|provider| provider.bearer_token.is_some())
    {
        return Err(inline_credential_error());
    }
    Ok(())
}

/// 安全诊断：不包含凭据正文，只说明当前 schema 禁止内联 token。
fn inline_credential_error() -> PureError {
    PureError::ConfigError(
        "schema 18 forbids inline provider bearer_token; use the Studio credential store"
            .to_string(),
    )
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

    fn assert_no_backup_files(store: &ConfigStore) {
        let dir = store.paths().config_dir();
        if !dir.exists() {
            return;
        }
        for entry in fs::read_dir(dir).unwrap().flatten() {
            assert!(
                !entry.file_name().to_string_lossy().contains(".bak"),
                "unexpected config backup: {}",
                entry.path().display()
            );
        }
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
            let config = store.load_or_default().unwrap();
            assert_eq!(
                fs::read_to_string(store.paths().config_file()).unwrap(),
                original
            );
            assert_eq!(config, expected);
            store.save(&config).unwrap();
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
    fn old_schemas_without_migration_path_fail_and_preserve_file() {
        for schema_version in [15, 16, 17] {
            let store = test_store(&format!("schema-{schema_version}"));
            fs::create_dir_all(store.paths().config_dir()).unwrap();
            let legacy = legacy_config(schema_version);
            fs::write(store.paths().config_file(), &legacy).unwrap();

            let error = store.load_or_default().unwrap_err().to_string();

            assert!(error.contains("schema version"), "{error}");
            assert_eq!(
                fs::read_to_string(store.paths().config_file()).unwrap(),
                legacy
            );
            assert_no_backup_files(&store);
        }
    }

    #[test]
    fn future_schema_is_rejected_and_preserved_during_startup() {
        let store = test_store("future-schema");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let future = toml::to_string_pretty(&StudioConfig::default_config())
            .unwrap()
            .replace("schema_version = 18", "schema_version = 4294967295");
        fs::write(store.paths().config_file(), &future).unwrap();

        let error = store.load_or_default().unwrap_err().to_string();

        assert!(error.contains("schema version"), "{error}");
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            future
        );
        assert_no_backup_files(&store);
    }

    #[test]
    fn malformed_config_is_rejected_and_preserved_during_startup() {
        let store = test_store("malformed");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), "not-toml").unwrap();

        let error = store.load_or_default().unwrap_err().to_string();

        assert!(error.contains("failed to parse Studio config"), "{error}");
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            "not-toml"
        );
        assert_no_backup_files(&store);
    }

    #[test]
    fn parse_error_does_not_leak_inline_credential_body() {
        let store = test_store("parse-error-secret");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let secret = "super-secret-token-value";
        let content = format!(
            "schema_version = 18\n\n[models.providers.deepseek]\nbearer_token = \"{secret}\" oops\n"
        );
        fs::write(store.paths().config_file(), &content).unwrap();

        let error = store.load_or_default().unwrap_err().to_string();

        assert!(!error.contains(secret), "{error}");
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            content
        );
    }

    #[test]
    fn inline_bearer_token_is_rejected_and_preserved_without_touching_credentials() {
        let credentials = Arc::new(RecordingCredentialStore::default());
        let store = ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home("inline-secret")),
            credentials.clone(),
        );
        let config = config_with_provider_secret("legacy-provider", "forbidden-secret");
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(
            store.paths().config_file(),
            toml::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
        let original = fs::read_to_string(store.paths().config_file()).unwrap();

        let error = store.load_or_default().unwrap_err().to_string();

        assert!(error.contains("bearer_token"), "{error}");
        assert!(!error.contains("forbidden-secret"), "{error}");
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            original
        );
        assert!(credentials.loads.lock().unwrap().is_empty());
        assert!(credentials.saves.lock().unwrap().is_empty());
        assert!(credentials.deletes.lock().unwrap().is_empty());
        assert_no_backup_files(&store);
    }

    #[test]
    fn invalid_current_schema_config_is_rejected_and_preserved_during_startup() {
        let store = test_store("invalid-current-schema");
        let mut invalid = StudioConfig::default_config();
        invalid.models.providers.clear();
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let invalid_toml = toml::to_string_pretty(&invalid).unwrap();
        fs::write(store.paths().config_file(), &invalid_toml).unwrap();

        let error = store.load_or_default().unwrap_err().to_string();

        assert!(!error.is_empty());
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            invalid_toml
        );
        assert_no_backup_files(&store);
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
    fn parse_failure_does_not_read_or_write_credentials() {
        let credentials = Arc::new(RecordingCredentialStore::default());
        let store = ConfigStore::with_credential_store(
            ConfigPaths::from_home(temp_home("parse-preserves-credentials")),
            credentials.clone(),
        );
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), "not-toml").unwrap();

        let error = store.load_or_default().unwrap_err().to_string();

        assert!(!error.is_empty());
        assert_eq!(
            fs::read_to_string(store.paths().config_file()).unwrap(),
            "not-toml"
        );
        assert!(credentials.loads.lock().unwrap().is_empty());
        assert!(credentials.saves.lock().unwrap().is_empty());
        assert!(credentials.deletes.lock().unwrap().is_empty());
        assert_no_backup_files(&store);
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

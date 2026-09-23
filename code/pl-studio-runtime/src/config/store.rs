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

    /// 判断 `config.toml` 是否真实缺失。
    ///
    /// 只认路径条目本身不存在（`NotFound`）为缺失；权限、元数据或符号链接异常一律返回
    /// `Err`，避免把“存在但不可访问/已损坏”的配置误判为缺失，从而静默落入默认配置或
    /// 在保存路径上覆盖未知文件。
    fn config_absent(&self) -> Result<bool> {
        match fs::symlink_metadata(self.paths.config_file()) {
            Ok(_) => Ok(false),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
            Err(error) => Err(self.config_path_error(error.into())),
        }
    }

    pub fn load_or_default(&self) -> Result<StudioConfig> {
        if self.config_absent()? {
            let mut config = StudioConfig::default_config();
            self.hydrate_credentials(&mut config)?;
            return Ok(config);
        }
        let content = fs::read_to_string(self.paths.config_file())
            .map_err(|error| self.config_path_error(error.into()))?;
        let mut config =
            parse_current_config(&content).map_err(|error| self.config_path_error(error))?;
        self.hydrate_credentials(&mut config)?;
        Ok(config)
    }

    /// 启动期读取并校验配置。
    ///
    /// 只有 `config.toml` 缺失时才采用内存默认配置。其余任何未知/未来版本、不可解析、
    /// 当前 schema 校验失败或含内联凭据的输入都保留原字节并返回错误，不重写文件、
    /// 也不替换为默认配置；已知 18/19 版本走数据保全式迁移路径。
    pub(crate) fn load_for_startup(&self) -> Result<StudioConfig> {
        if self.config_absent()? {
            let mut config = StudioConfig::default_config();
            self.hydrate_credentials(&mut config)?;
            return Ok(config);
        }
        let content = fs::read(self.paths.config_file())
            .map_err(|error| self.config_path_error(error.into()))?;
        // Version dispatch precedes the strict parse so the supported 18/19 configs reach
        // their data-preserving migration instead of failing current-schema validation.
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
            return self
                .migrate_legacy_config_for_startup(&content, version)
                .map_err(|error| self.config_path_error(error));
        }
        // Unknown/future versions, malformed TOML and current-schema validation failures
        // all fail closed here: the original bytes stay on disk untouched.
        let mut config =
            parse_startup_config(&content).map_err(|error| self.config_path_error(error))?;
        self.hydrate_credentials(&mut config)?;
        Ok(config)
    }

    /// 为配置文件错误附上 `config.toml` 路径以便定位。保持 `Io` 类别不变，只消化
    /// `PureError` 文本，不引入任何配置内容、token 或凭据。
    fn config_path_error(&self, error: PureError) -> PureError {
        match error {
            PureError::Io(error) => PureError::Io(std::io::Error::new(
                error.kind(),
                format!(
                    "failed to access Studio config {}: {error}",
                    self.paths.config_file().display()
                ),
            )),
            PureError::ConfigError(message) => PureError::ConfigError(format!(
                "failed to load Studio config {}: {message}",
                self.paths.config_file().display()
            )),
            other => other,
        }
    }

    fn migrate_legacy_config_for_startup(
        &self,
        original: &[u8],
        source_version: u32,
    ) -> Result<StudioConfig> {
        self.migrate_legacy_config_for_startup_with(original, source_version, |path, content| {
            pl_tool::workspace::write_file_atomically(path, content).map_err(Into::into)
        })
    }

    fn migrate_legacy_config_for_startup_with(
        &self,
        original: &[u8],
        source_version: u32,
        replace: impl FnOnce(&Path, &[u8]) -> Result<()>,
    ) -> Result<StudioConfig> {
        let text = std::str::from_utf8(original).map_err(|error| {
            PureError::ConfigError(format!("invalid schema {source_version} UTF-8: {error}"))
        })?;
        let mut config = parse_typed_config(text)?;
        reject_inline_credentials(&config)?;
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
        Ok(config)
    }

    pub fn load(&self) -> Result<StudioConfig> {
        let content = fs::read_to_string(self.paths.config_file())
            .map_err(|error| self.config_path_error(error.into()))?;
        let mut config =
            parse_current_config(&content).map_err(|error| self.config_path_error(error))?;
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
        if !self.config_absent()? {
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
        if self.config_absent()? {
            return Ok(BTreeSet::new());
        }
        let content = fs::read_to_string(self.paths.config_file())
            .map_err(|error| self.config_path_error(error.into()))?;
        let persisted =
            parse_current_config(&content).map_err(|error| self.config_path_error(error))?;
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
    parse_config(content)
}

fn parse_startup_config(content: &[u8]) -> Result<StudioConfig> {
    let content = std::str::from_utf8(content).map_err(|error| {
        PureError::ConfigError(format!("failed to parse Studio config as UTF-8: {error}"))
    })?;
    parse_config(content)
}

fn parse_config(content: &str) -> Result<StudioConfig> {
    let config = parse_typed_config(content)?;
    reject_inline_credentials(&config)?;
    config.validate()?;
    Ok(config)
}

fn parse_typed_config(content: &str) -> Result<StudioConfig> {
    // 第一遍只做 TOML 语法解析；语法诊断由解析器生成，不含文档内容。
    if let Err(error) = toml::from_str::<toml::Table>(content) {
        return Err(PureError::ConfigError(format!(
            "invalid Studio config TOML{}",
            toml_error_location(content, &error)
        )));
    }
    // 第二遍做结构/类型解码；serde 诊断可能回显字段值，因此只保留类别与行/列位置。
    toml::from_str(content).map_err(|error| {
        PureError::ConfigError(format!(
            "Studio config TOML does not match the current schema{}",
            toml_error_location(content, &error)
        ))
    })
}

/// 仅按错误的字节 span 计算 1-based 行/列用于定位；刻意不回显任何原文或字段值。
fn toml_error_location(content: &str, error: &toml::de::Error) -> String {
    let Some(span) = error.span() else {
        return String::new();
    };
    let Some(prefix) = content.get(..span.start) else {
        return String::new();
    };
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rfind('\n')
        .map_or(prefix.len(), |newline| prefix.len() - newline - 1)
        + 1;
    format!(" at line {line}, column {column}")
}

fn reject_inline_credentials(config: &StudioConfig) -> Result<()> {
    if config
        .models
        .providers
        .values()
        .any(|provider| provider.bearer_token.is_some())
    {
        return Err(PureError::ConfigError(
            "current schema forbids inline provider bearer_token; use the Studio credential store"
                .to_string(),
        ));
    }
    Ok(())
}

fn write_config_backup(config_path: &Path, content: &[u8], kind: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    write_config_backup_at(config_path, content, kind, stamp)
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

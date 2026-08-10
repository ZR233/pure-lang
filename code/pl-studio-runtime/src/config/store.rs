use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_core::{ProviderId, builtin_provider_catalog};
use serde::Deserialize;

use crate::{PureError, Result};

use super::migration::{PREVIOUS_STUDIO_CONFIG_SCHEMA_VERSION, migrate_v12, schema_version};
use super::{STUDIO_CONFIG_DIR_NAME, STUDIO_CONFIG_FILE_NAME, StudioConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    config_dir: PathBuf,
    config_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    paths: ConfigPaths,
}

impl ConfigPaths {
    pub fn for_current_user() -> Result<Self> {
        Ok(Self::from_home(user_home_dir()?))
    }

    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let config_dir = home.into().join(STUDIO_CONFIG_DIR_NAME);
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
        Ok(Self::new(ConfigPaths::for_current_user()?))
    }

    pub fn new(paths: ConfigPaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &ConfigPaths {
        &self.paths
    }

    pub fn config_exists(&self) -> bool {
        self.paths.config_file().exists()
    }

    pub fn load_or_default(&self) -> Result<StudioConfig> {
        if !self.config_exists() {
            return Ok(StudioConfig::default_config());
        }
        let content = fs::read_to_string(self.paths.config_file())?;
        match schema_version(&content) {
            Ok(PREVIOUS_STUDIO_CONFIG_SCHEMA_VERSION) => match migrate_v12(&content) {
                Ok(migration) => {
                    self.save(&migration.config)?;
                    for diagnostic in migration.diagnostics {
                        tracing::warn!(diagnostic, "Studio 配置已从 schema 12 迁移到 schema 13");
                    }
                    Ok(migration.config)
                }
                Err(error) => {
                    tracing::warn!(%error, "Studio schema 12 配置迁移失败，将按拒绝配置处理");
                    self.replace_rejected_config()
                }
            },
            Ok(_) => match parse_current_config(&content) {
                Ok(config) => Ok(config),
                Err(error) => {
                    tracing::warn!(%error, "Studio 配置不兼容，将归档并重建");
                    self.replace_rejected_config()
                }
            },
            Err(error) => {
                tracing::warn!(%error, "Studio 配置无法读取 schema，将归档并重建");
                self.replace_rejected_config()
            }
        }
    }

    pub fn load(&self) -> Result<StudioConfig> {
        let content = fs::read_to_string(self.paths.config_file())?;
        parse_current_config(&content)
    }

    pub fn save(&self, config: &StudioConfig) -> Result<()> {
        config.validate()?;
        let content = toml::to_string_pretty(config).map_err(|error| {
            PureError::ConfigError(format!("failed to serialize Studio config: {error}"))
        })?;
        fs::create_dir_all(self.paths.config_dir())?;
        let temporary = temporary_path(self.paths.config_file());
        let result = (|| -> Result<()> {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            replace_file_atomically(&temporary, self.paths.config_file())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
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

    fn replace_rejected_config(&self) -> Result<StudioConfig> {
        let rejected_content = fs::read_to_string(self.paths.config_file())?;
        let credentials = toml::from_str::<RejectedConfigCredentials>(&rejected_content).ok();
        let backup = rejected_backup_path(self.paths.config_file());
        fs::copy(self.paths.config_file(), &backup)?;
        fs::remove_file(self.paths.config_file())?;

        let mut config = StudioConfig::default_config();
        if let Some(credentials) = credentials {
            restore_provider_credentials(&mut config, credentials);
        }
        self.save(&config)?;
        Ok(config)
    }
}

fn parse_current_config(content: &str) -> Result<StudioConfig> {
    let config: StudioConfig = toml::from_str(content).map_err(|error| {
        PureError::ConfigError(format!("failed to parse Studio config: {error}"))
    })?;
    config.validate()?;
    Ok(config)
}

#[derive(Debug, Default, Deserialize)]
struct RejectedConfigCredentials {
    #[serde(default)]
    models: RejectedModelCredentials,
}

#[derive(Debug, Default, Deserialize)]
struct RejectedModelCredentials {
    #[serde(default)]
    providers: BTreeMap<String, ProviderCredentials>,
}

#[derive(Debug, Default, Deserialize)]
struct ProviderCredentials {
    #[serde(default)]
    bearer_token: Option<String>,
    #[serde(default)]
    bearer_token_env: Option<String>,
}

fn restore_provider_credentials(config: &mut StudioConfig, credentials: RejectedConfigCredentials) {
    let presets = builtin_provider_catalog().presets;
    for (provider_id, credentials) in credentials.models.providers {
        let Ok(provider_id) = ProviderId::new(provider_id) else {
            continue;
        };
        if !config.models.providers.contains_key(&provider_id)
            && let Some(preset) = presets
                .iter()
                .find(|preset| preset.id.as_str() == provider_id.as_str())
        {
            config
                .models
                .providers
                .insert(provider_id.clone(), preset.provider.clone());
        }
        let Some(provider) = config.models.providers.get_mut(&provider_id) else {
            continue;
        };
        if credentials.bearer_token.is_some() {
            provider.bearer_token = credentials.bearer_token;
        }
        if credentials.bearer_token_env.is_some() {
            provider.bearer_token_env = credentials.bearer_token_env;
        }
    }
}

fn rejected_backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(STUDIO_CONFIG_FILE_NAME);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!("{file_name}.rejected.{stamp}.bak"))
}

fn temporary_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_extension(format!("tmp-{}-{stamp}", std::process::id()))
}

fn replace_file_atomically(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let source = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let target = target
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: both path buffers are NUL-terminated and remain alive for the duration of the
        // call. The temporary file lives beside the target, so replacement stays on one volume.
        let replaced = unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if replaced == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        fs::rename(source, target)
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::config::STUDIO_CONFIG_SCHEMA_VERSION;

    const DEEPSEEK_V12: &str = r#"
schema_version = 12

[models.providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"

[models.providers.deepseek.transport]
source = "preset"
preset = "deepseek"
connection_mode = "http"

[models.providers.deepseek.catalog]
source = "bundled"
catalog = "deepseek"

[[models.providers.deepseek.catalog.additional_models]]
slug = "legacy-deepseek-extra"
display_name = "Legacy DeepSeek Extra"

[models.routes.explorer]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[models.routes.planner]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[models.routes.executor]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[models.routes.reviewer]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"
"#;

    const CUSTOM_RESPONSES_V12: &str = r#"
schema_version = 12

[models.providers.proxy]
name = "Responses Proxy"
base_url = "https://proxy.example/v1"
bearer_token = "preserved-secret"

[models.providers.proxy.http_headers]
x-test = "preserved"

[models.providers.proxy.transport]
source = "custom"
protocol = "responses"
connection_mode = "http"

[models.providers.proxy.catalog]
source = "explicit"

[[models.providers.proxy.catalog.models]]
slug = "proxy-model"
display_name = "Proxy Model"

[models.routes.explorer]
provider = "proxy"
model = "proxy-model"

[models.routes.planner]
provider = "proxy"
model = "proxy-model"

[models.routes.executor]
provider = "proxy"
model = "proxy-model"

[models.routes.reviewer]
provider = "proxy"
model = "proxy-model"
"#;

    const OPENAI_HTTP_V12: &str = r#"
schema_version = 12

[models.providers.openai]
name = "OpenAI HTTP"
base_url = "https://api.openai.com/v1"

[models.providers.openai.transport]
source = "preset"
preset = "openai"
connection_mode = "http"

[models.providers.openai.catalog]
source = "bundled"
catalog = "openai"

[models.routes.explorer]
provider = "openai"
model = "gpt-5.6-sol"
effort = "low"

[models.routes.planner]
provider = "openai"
model = "gpt-5.6-sol"
effort = "low"

[models.routes.executor]
provider = "openai"
model = "gpt-5.6-sol"
effort = "low"

[models.routes.reviewer]
provider = "openai"
model = "gpt-5.6-sol"
effort = "low"
"#;

    fn temp_home(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after Unix epoch")
            .as_nanos();
        env::temp_dir().join(format!(
            "pl-studio-config-{name}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[test]
    fn rejected_document_is_archived_and_only_provider_credentials_are_restored() {
        let store = ConfigStore::new(ConfigPaths::from_home(temp_home("rejected")));
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        let rejected = r#"
schema_version = 11

[models.providers.openai]
bearer_token = "existing-secret"
bearer_token_env = "ORIGINAL_OPENAI_API_KEY"

[models.routes.planner]
provider = "openai"
model = "obsolete-model"
reasoning_effort = "medium"
"#;
        fs::write(store.paths().config_file(), rejected).unwrap();

        let config = store.load_or_default().unwrap();

        assert_eq!(config.schema_version, STUDIO_CONFIG_SCHEMA_VERSION);
        let openai = &config.models.providers[&ProviderId::new("openai").unwrap()];
        assert_eq!(openai.bearer_token.as_deref(), Some("existing-secret"));
        assert_eq!(
            openai.bearer_token_env.as_deref(),
            Some("ORIGINAL_OPENAI_API_KEY")
        );
        assert_ne!(
            config.models.routes[&crate::StudioRole::Planner.id()].model,
            "obsolete-model"
        );
        assert_eq!(store.load().unwrap(), config);

        let backup = fs::read_dir(store.paths().config_dir())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.rejected.")
            })
            .expect("rejected config backup");
        assert_eq!(fs::read_to_string(backup.path()).unwrap(), rejected);
    }

    #[test]
    fn schema_12_bundled_models_migrate_to_the_model_transport_matrix() {
        let store = ConfigStore::new(ConfigPaths::from_home(temp_home("migrate-bundled")));
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), DEEPSEEK_V12).unwrap();

        let config = store.load_or_default().unwrap();

        assert_eq!(config.schema_version, STUDIO_CONFIG_SCHEMA_VERSION);
        let provider = &config.models.providers[&ProviderId::new("deepseek").unwrap()];
        assert_eq!(provider.preset_id().unwrap().as_str(), "deepseek");
        let flash = provider.to_provider_info("deepseek-v4-flash").unwrap();
        let pro = provider.to_provider_info("deepseek-v4-pro").unwrap();
        assert_eq!(flash.protocol, pl_model::ProviderWireProtocol::Responses);
        assert_eq!(
            pro.protocol,
            pl_model::ProviderWireProtocol::ChatCompletions
        );
        assert_eq!(
            flash.connection_mode,
            pl_model::ProviderConnectionMode::Http
        );
        assert_eq!(pro.connection_mode, pl_model::ProviderConnectionMode::Http);
        let additional = provider
            .declared_models()
            .unwrap()
            .into_iter()
            .find(|model| model.slug == "legacy-deepseek-extra")
            .unwrap();
        assert_eq!(
            additional.transport,
            pl_model::ModelTransportProfile::chat_completions_http()
        );
        assert_eq!(store.load().unwrap(), config);
        assert!(
            fs::read_to_string(store.paths().config_file())
                .unwrap()
                .starts_with("schema_version = 13")
        );
    }

    #[test]
    fn schema_12_custom_models_inherit_provider_transport_and_roundtrip() {
        let store = ConfigStore::new(ConfigPaths::from_home(temp_home("migrate-custom")));
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), CUSTOM_RESPONSES_V12).unwrap();

        let config = store.load_or_default().unwrap();

        let provider = &config.models.providers[&ProviderId::new("proxy").unwrap()];
        assert!(provider.preset_id().is_none());
        assert_eq!(provider.bearer_token.as_deref(), Some("preserved-secret"));
        assert_eq!(
            provider.http_headers.as_ref().unwrap().get("x-test"),
            Some(&"preserved".to_string())
        );
        let model = provider.declared_models().unwrap().remove(0);
        assert_eq!(
            model.transport.protocol,
            pl_model::ProviderWireProtocol::Responses
        );
        assert_eq!(
            model.transport.supported_connection_modes,
            vec![
                pl_model::ProviderConnectionMode::WebSocket,
                pl_model::ProviderConnectionMode::Http,
            ]
        );
        assert_eq!(
            model.transport.default_connection_mode,
            pl_model::ProviderConnectionMode::Http
        );
        assert_eq!(store.load().unwrap(), config);
    }

    #[test]
    fn schema_12_openai_http_selection_becomes_per_model_overrides() {
        let store = ConfigStore::new(ConfigPaths::from_home(temp_home("migrate-openai-http")));
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(store.paths().config_file(), OPENAI_HTTP_V12).unwrap();

        let config = store.load_or_default().unwrap();

        let provider = &config.models.providers[&ProviderId::new("openai").unwrap()];
        for model in provider.effective_models().unwrap() {
            assert_eq!(
                model.transport.protocol,
                pl_model::ProviderWireProtocol::Responses
            );
            assert_eq!(
                model.transport.default_connection_mode,
                pl_model::ProviderConnectionMode::Http,
                "{} should preserve the schema 12 HTTP selection",
                model.slug
            );
        }
        assert_eq!(provider.connection_overrides().len(), 6);
    }

    #[test]
    fn schema_12_chat_websocket_falls_back_to_http_with_a_diagnostic() {
        let invalid = CUSTOM_RESPONSES_V12
            .replace(
                "protocol = \"responses\"",
                "protocol = \"chat_completions\"",
            )
            .replace(
                "connection_mode = \"http\"",
                "connection_mode = \"web_socket\"",
            );

        let migration = super::super::migration::migrate_v12(&invalid).unwrap();

        assert!(!migration.diagnostics.is_empty());
        let provider = &migration.config.models.providers[&ProviderId::new("proxy").unwrap()];
        let model = provider.declared_models().unwrap().remove(0);
        assert_eq!(
            model.transport,
            pl_model::ModelTransportProfile::chat_completions_http()
        );
    }
}

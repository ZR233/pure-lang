use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_core::{ProviderId, builtin_provider_catalog};
use serde::Deserialize;

use crate::{PureError, Result};

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
        match self.load() {
            Ok(config) => Ok(config),
            Err(_) => self.replace_rejected_config(),
        }
    }

    pub fn load(&self) -> Result<StudioConfig> {
        let content = fs::read_to_string(self.paths.config_file())?;
        let config: StudioConfig = toml::from_str(&content).map_err(|error| {
            PureError::ConfigError(format!("failed to parse Studio config: {error}"))
        })?;
        config.validate()?;
        Ok(config)
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
            fs::rename(&temporary, self.paths.config_file())?;
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
}

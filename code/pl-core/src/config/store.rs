use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_protocol::{PureError, Result};

use super::{CONFIG_DIR_NAME, CONFIG_FILE_NAME, PureConfig};

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
        let home = user_home_dir()?;
        Ok(Self::from_home(home))
    }

    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let config_dir = home.into().join(CONFIG_DIR_NAME);
        let config_file = config_dir.join(CONFIG_FILE_NAME);
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

    pub fn load_or_default(&self) -> Result<PureConfig> {
        if !self.config_exists() {
            return Ok(PureConfig::default_config());
        }

        self.load()
            .or_else(|_| self.backup_invalid_and_save_default())
    }

    pub fn load(&self) -> Result<PureConfig> {
        let content = fs::read_to_string(self.paths.config_file())?;
        PureConfig::from_toml(&content)
    }

    pub fn save(&self, config: &PureConfig) -> Result<()> {
        config.validate()?;
        fs::create_dir_all(self.paths.config_dir())?;
        fs::write(self.paths.config_file(), config.to_toml_pretty()?)?;
        Ok(())
    }

    pub fn init_default(&self) -> Result<PureConfig> {
        if self.paths.config_file().exists() {
            return Err(PureError::ConfigError(format!(
                "config already exists: {}",
                self.paths.config_file().display()
            )));
        }

        let config = PureConfig::default_config();
        self.save(&config)?;
        Ok(config)
    }

    fn backup_invalid_and_save_default(&self) -> Result<PureConfig> {
        let backup_file = self.invalid_config_backup_file();
        fs::copy(self.paths.config_file(), &backup_file).map_err(|error| {
            PureError::ConfigError(format!(
                "failed to backup invalid config to {}: {error}",
                backup_file.display()
            ))
        })?;

        let config = PureConfig::default_config();
        self.save(&config)?;
        Ok(config)
    }

    fn invalid_config_backup_file(&self) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let config_dir = self.paths.config_dir();
        let mut candidate = config_dir.join(format!("config.invalid.backup.{stamp}.toml"));
        for index in 2.. {
            if !candidate.exists() {
                return candidate;
            }
            candidate = config_dir.join(format!("config.invalid.backup.{stamp}.{index}.toml"));
        }
        candidate
    }
}

fn user_home_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];

    HOME_VARS
        .iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| PureError::ConfigError("could not resolve user home directory".to_string()))
}

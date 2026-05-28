use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::CONFIG_DIR_NAME;
use crate::studio::ids::unix_seconds;

const STUDIO_DIR_NAME: &str = "studio";
const STUDIO_DB_FILE_NAME: &str = "studio_2.sqlite";
const LEGACY_STUDIO_DB_FILE_NAME: &str = "studio_1.sqlite";

pub fn prepare_database_switch() -> Result<PathBuf> {
    let target = default_db_path()?;
    let legacy = legacy_db_path()?;
    if target.exists() || !legacy.exists() {
        return Ok(target);
    }

    let now = unix_seconds();
    let backup = legacy.with_extension(format!("sqlite.v1.backup.{now}"));
    fs::copy(&legacy, &backup).with_context(|| {
        format!(
            "failed to backup legacy studio db from {} to {}",
            legacy.display(),
            backup.display()
        )
    })?;
    fs::remove_file(&legacy)
        .with_context(|| format!("failed to remove legacy studio db: {}", legacy.display()))?;
    Ok(target)
}

pub fn default_db_path() -> Result<PathBuf> {
    Ok(user_home_dir()?
        .join(CONFIG_DIR_NAME)
        .join(STUDIO_DIR_NAME)
        .join(STUDIO_DB_FILE_NAME))
}

fn legacy_db_path() -> Result<PathBuf> {
    Ok(user_home_dir()?
        .join(CONFIG_DIR_NAME)
        .join(STUDIO_DIR_NAME)
        .join(LEGACY_STUDIO_DB_FILE_NAME))
}

fn user_home_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];

    HOME_VARS
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .context("could not resolve user home directory")
}

pub fn sqlite_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("sqlite://{path}?mode=rwc")
}

pub fn project_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

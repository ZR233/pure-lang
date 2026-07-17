use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::CONFIG_DIR_NAME;

const STUDIO_DIR_NAME: &str = "studio";
const STUDIO_DB_FILE_NAME: &str = "studio_2.sqlite";

pub fn default_db_path() -> Result<PathBuf> {
    Ok(user_home_dir()?
        .join(CONFIG_DIR_NAME)
        .join(STUDIO_DIR_NAME)
        .join(STUDIO_DB_FILE_NAME))
}

pub fn default_attachments_dir() -> Result<PathBuf> {
    Ok(user_home_dir()?
        .join(CONFIG_DIR_NAME)
        .join(STUDIO_DIR_NAME)
        .join("attachments"))
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

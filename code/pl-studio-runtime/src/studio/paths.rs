use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::CONFIG_DIR_NAME;

const STUDIO_DIR_NAME: &str = "studio";
const DATABASE_FILE_NAME: &str = "studio.sqlite";
const STUDIO_HOME_ENV: &str = "PURE_STUDIO_HOME";

pub fn default_db_path() -> Result<PathBuf> {
    Ok(studio_dir()?.join(DATABASE_FILE_NAME))
}

pub fn default_attachments_dir() -> Result<PathBuf> {
    Ok(studio_dir()?.join("attachments"))
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

fn studio_dir() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os(STUDIO_HOME_ENV) {
        let path = PathBuf::from(value);
        if path.as_os_str().is_empty() {
            anyhow::bail!("{STUDIO_HOME_ENV} must not be empty when configured");
        }
        if !path.is_absolute() {
            anyhow::bail!("{STUDIO_HOME_ENV} must be an absolute path");
        }
        return Ok(path);
    }
    Ok(user_home_dir()?.join(CONFIG_DIR_NAME).join(STUDIO_DIR_NAME))
}

pub fn sqlite_url(path: &Path) -> String {
    sqlite_url_with_mode(path, "rwc")
}

pub(crate) fn sqlite_read_only_url(path: &Path) -> String {
    sqlite_url_with_mode(path, "ro")
}

pub fn project_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn sqlite_url_with_mode(path: &Path, mode: &str) -> String {
    let path = path.to_string_lossy();
    let path = path
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!("//{path}"))
        .or_else(|| path.strip_prefix(r"\\?\").map(ToOwned::to_owned))
        .unwrap_or_else(|| path.into_owned())
        .replace('\\', "/");
    format!("sqlite://{path}?mode={mode}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_url_removes_windows_verbatim_drive_prefix() {
        assert_eq!(
            sqlite_url(Path::new(r"\\?\C:\studio\studio.sqlite")),
            "sqlite://C:/studio/studio.sqlite?mode=rwc"
        );
    }

    #[test]
    fn sqlite_url_converts_windows_verbatim_unc_prefix() {
        assert_eq!(
            sqlite_read_only_url(Path::new(r"\\?\UNC\server\share\studio\studio.sqlite")),
            "sqlite:////server/share/studio/studio.sqlite?mode=ro"
        );
    }
}

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::STUDIO_CONFIG_DIR_NAME;

const STUDIO_DIR_NAME: &str = "studio";
const DATABASE_FILE_NAME: &str = "studio.sqlite";
const SESSIONS_DIR_NAME: &str = "sessions";
const SKILLS_DIR_NAME: &str = "skills";
const SYSTEM_SKILLS_DIR_NAME: &str = ".system";
const STUDIO_HOME_ENV: &str = "ANYWORK_HOME";
/// Longest accepted Thread identity; ids are single path segments.
const MAX_STORAGE_ID_BYTES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioPaths {
    home: PathBuf,
    data_dir: PathBuf,
}

impl StudioPaths {
    pub fn resolve(explicit_home: Option<PathBuf>) -> Result<Self> {
        let home = match explicit_home {
            Some(home) => validate_studio_home(home, "--studio-home")?,
            None => match std::env::var_os(STUDIO_HOME_ENV) {
                Some(home) => validate_studio_home(PathBuf::from(home), STUDIO_HOME_ENV)?,
                None => user_home_dir()?.join(STUDIO_CONFIG_DIR_NAME),
            },
        };
        Ok(Self {
            data_dir: home.join(STUDIO_DIR_NAME),
            home,
        })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn database(&self) -> PathBuf {
        self.data_dir.join(DATABASE_FILE_NAME)
    }

    pub fn runtime_lock(&self) -> PathBuf {
        self.data_dir.join("runtime.lock")
    }

    /// Returns the product-owned directory materialized from bundled system Skills.
    pub fn system_skills_dir(&self) -> PathBuf {
        self.data_dir
            .join(SKILLS_DIR_NAME)
            .join(SYSTEM_SKILLS_DIR_NAME)
    }
}

/// Directory holding one SQLite session database per root/child Thread,
/// resolved beside the product `studio.sqlite`.
pub fn sessions_dir_beside(database: &Path) -> PathBuf {
    database
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(SESSIONS_DIR_NAME)
}

/// Derives one session database path under `sessions_dir`, rejecting traversal.
///
/// The Thread id is the only input, so root and child Threads each own a
/// distinct database file and no caller can address a database outside
/// `sessions_dir`.
///
/// # Errors
/// Rejects an id that is empty, oversized, contains path separators or
/// traversal, or whose derived parent escapes `sessions_dir`.
pub fn session_database_path(sessions_dir: &Path, thread_id: &str) -> Result<PathBuf> {
    storage_path(sessions_dir, "Thread", thread_id, "sqlite")
}

fn storage_path(base: &Path, kind: &str, id: &str, extension: &str) -> Result<PathBuf> {
    validate_storage_id(kind, id)?;
    let path = base.join(format!("{id}.{extension}"));
    if path.parent() != Some(base) {
        bail!("{kind} id must resolve inside its storage directory: {id}");
    }
    Ok(path)
}

pub(crate) fn validate_storage_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty() || id.len() > MAX_STORAGE_ID_BYTES {
        bail!("{kind} id must be 1..={MAX_STORAGE_ID_BYTES} bytes");
    }
    if !id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        bail!("{kind} id must use only ASCII letters, digits, '-', '_' or '.': {id}");
    }
    if id == "." || id == ".." || id.contains("..") {
        bail!("{kind} id must not traverse directories: {id}");
    }
    Ok(())
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

fn validate_studio_home(path: PathBuf, source: &str) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        anyhow::bail!("{source} must not be empty when configured");
    }
    if !path.is_absolute() {
        anyhow::bail!("{source} must be an absolute path");
    }
    Ok(path)
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

    #[test]
    fn explicit_home_keeps_config_and_runtime_under_one_root() {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\isolated\.anywork")
        } else {
            PathBuf::from("/tmp/isolated/.anywork")
        };
        let paths = StudioPaths::resolve(Some(root.clone())).unwrap();

        assert_eq!(paths.home(), root);
        assert_eq!(paths.database(), root.join("studio").join("studio.sqlite"));
        assert_eq!(
            paths.runtime_lock(),
            root.join("studio").join("runtime.lock")
        );
        assert_eq!(
            paths.system_skills_dir(),
            root.join("studio").join("skills").join(".system")
        );
    }

    #[test]
    fn session_paths_sit_beside_the_product_database_and_validate_identities() {
        let root = PathBuf::from("/tmp/isolated/.anywork");
        let database = root.join("studio").join("studio.sqlite");
        let sessions = sessions_dir_beside(&database);

        assert_eq!(sessions, root.join("studio").join("sessions"));
        assert_eq!(
            session_database_path(&sessions, "thread-abc-1").unwrap(),
            root.join("studio")
                .join("sessions")
                .join("thread-abc-1.sqlite")
        );
    }

    #[test]
    fn storage_paths_reject_unsafe_identities() {
        let sessions = PathBuf::from("/tmp/isolated/.anywork/studio/sessions");

        for id in [
            "",
            ".",
            "..",
            "../escape",
            "a/b",
            "..\\escape",
            "thread:1",
            "thread id",
            "thread\0id",
        ] {
            assert!(
                session_database_path(&sessions, id).is_err(),
                "session id {id:?} must be rejected"
            );
        }
    }
}

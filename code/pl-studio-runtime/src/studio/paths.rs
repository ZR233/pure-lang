use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::config::STUDIO_CONFIG_DIR_NAME;

const STUDIO_DIR_NAME: &str = "studio";
const SESSION_LAYOUT_VERSION: &str = "v2";
const DATABASE_FILE_NAME: &str = "studio.sqlite";
const SKILLS_DIR_NAME: &str = "skills";
const SYSTEM_SKILLS_DIR_NAME: &str = ".system";
const STUDIO_HOME_ENV: &str = "ANYWORK_HOME";
const CALLS_FILE_NAME: &str = "calls.sqlite";
const CONFIG_FILE_NAME: &str = "config.toml";
const SETTINGS_FILE_NAME: &str = "settings.toml";
const WORKSPACES_FILE_NAME: &str = "workspaces.toml";
const CATALOG_FILE_NAME: &str = "catalog.toml";
const AGENTS_DIR_NAME: &str = "agents";
const SESSIONS_DIR_NAME: &str = "sessions";
const CALLS_DIR_NAME: &str = "calls";
const MIGRATIONS_DIR_NAME: &str = "migrations";
const ATTACHMENT_DRAFTS_DIR_NAME: &str = "attachment-drafts";

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
            data_dir: home.join(STUDIO_DIR_NAME).join(SESSION_LAYOUT_VERSION),
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
        self.home.join(STUDIO_DIR_NAME).join("runtime.lock")
    }

    /// Returns the product-owned directory materialized from bundled system Skills.
    pub fn system_skills_dir(&self) -> PathBuf {
        self.home
            .join(STUDIO_DIR_NAME)
            .join(SKILLS_DIR_NAME)
            .join(SYSTEM_SKILLS_DIR_NAME)
    }
}

/// Layout entry points fixed by `design/17` §17.1.
///
/// Every slice (store, config, catalog) resolves its location here instead of
/// re-deriving names. Entries that no slice consumes yet are marked `dead_code` so a mid-refactor
/// tree still builds under `-D warnings`.
#[allow(dead_code)]
impl StudioPaths {
    /// Data root holding the versioned session and call databases.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Global provider/model/Skill configuration (`design/17` §17.1).
    pub fn config_file(&self) -> PathBuf {
        self.home.join(CONFIG_FILE_NAME)
    }

    /// Product and UI settings store.
    pub fn settings_file(&self) -> PathBuf {
        self.versioned_home().join(SETTINGS_FILE_NAME)
    }

    /// Project/Workspace definitions and stable references.
    pub fn workspaces_file(&self) -> PathBuf {
        self.versioned_home().join(WORKSPACES_FILE_NAME)
    }

    /// Lightweight Thread directory summaries consumed by the first screen.
    pub fn catalog_file(&self) -> PathBuf {
        self.versioned_home().join(CATALOG_FILE_NAME)
    }

    /// Config runtime path view for the same product home.
    ///
    /// `config.toml` and `agents/*.toml` are product files under the resolved Studio home; the
    /// config runtime consumes this view instead of re-deriving the layout.
    pub fn config_paths(&self) -> crate::config::ConfigPaths {
        crate::config::ConfigPaths::from_config_dir(self.home.clone())
    }

    /// One stable TOML file per user Agent Profile.
    pub fn agents_dir(&self) -> PathBuf {
        self.home.join(AGENTS_DIR_NAME)
    }

    /// Root of per-Thread session directories; one child owns `state.toml`, `history.sqlite` and
    /// `blobs`. Startup loads the directory listing only; individual Threads activate lazily.
    pub fn sessions_dir(&self) -> PathBuf {
        self.versioned_home().join(SESSIONS_DIR_NAME)
    }

    /// Global per-call record directory (`calls.sqlite` plus blob references).
    pub fn calls_dir(&self) -> PathBuf {
        self.versioned_home().join(CALLS_DIR_NAME)
    }

    /// Global, disposable call statistics database.
    pub fn calls_database(&self) -> PathBuf {
        self.calls_dir().join(CALLS_FILE_NAME)
    }

    /// Version-local migration state and consistent backups.
    pub fn migrations_dir(&self) -> PathBuf {
        self.versioned_home().join(MIGRATIONS_DIR_NAME)
    }

    /// Temporary attachment-draft root. Persistent attachments live under the owning Thread's
    /// `blobs` directory; this root is never an attachment catalog or content-addressed store.
    pub fn attachment_drafts_dir(&self) -> PathBuf {
        self.data_dir.join(ATTACHMENT_DRAFTS_DIR_NAME)
    }

    /// Stable on-disk directory for one Thread session.
    ///
    /// The layout and normal store must agree on this mapping.
    pub fn thread_storage_dir(&self, thread_id: &str) -> PathBuf {
        self.sessions_dir().join(thread_storage_key(thread_id))
    }

    /// Content-addressed blob root for one Thread session.
    ///
    /// Keeps attachments under the owning session root
    /// (`sessions/<storage-key>/blobs`, `design/17` §17.1); every consumer resolves it here so the
    /// attachment catalog path and the blob path can never diverge.
    pub fn thread_blobs_dir(&self, thread_id: &str) -> PathBuf {
        self.thread_storage_dir(thread_id).join("blobs")
    }

    fn versioned_home(&self) -> PathBuf {
        self.home.join(SESSION_LAYOUT_VERSION)
    }
}

/// Stable on-disk key for a Thread session directory.
pub fn thread_storage_key(thread_id: &str) -> String {
    hex::encode(Sha256::digest(thread_id.as_bytes()))
}

pub fn default_db_path() -> Result<PathBuf> {
    Ok(StudioPaths::resolve(None)?.database())
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

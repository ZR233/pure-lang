use std::env;
use std::path::{Component, Path, PathBuf};

use pl_protocol::{PureError, Result};

pub(super) fn safe_relative_path(path: &str) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PureError::ConfigError(format!(
                    "path must be relative and stay inside its root: {path}"
                )));
            }
        }
    }
    if result.as_os_str().is_empty() {
        return Err(PureError::ConfigError("path must not be empty".to_string()));
    }
    Ok(result)
}

pub(super) fn category_path(category: &str) -> Result<PathBuf> {
    let relative = safe_relative_path(category.trim())?;
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(PureError::ConfigError(
                "skill category must contain normal path components".to_string(),
            ));
        };
        let Some(part) = part.to_str() else {
            return Err(PureError::ConfigError(
                "skill category must be valid UTF-8".to_string(),
            ));
        };
        if !part
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        {
            return Err(PureError::ConfigError(format!(
                "skill category contains unsupported characters: {category}"
            )));
        }
    }
    Ok(relative)
}

pub(super) fn expand_home(path: &str) -> Result<PathBuf> {
    if path == "~" {
        return user_home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        return Ok(user_home_dir()?.join(rest));
    }
    Ok(PathBuf::from(path))
}

pub(super) fn user_home_dir() -> Result<PathBuf> {
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

pub(super) fn normalized_platforms(platforms: Vec<String>) -> Vec<String> {
    platforms
        .into_iter()
        .map(|platform| normalize_platform(&platform))
        .filter(|platform| !platform.is_empty())
        .collect()
}

fn normalize_platform(platform: &str) -> String {
    match platform.trim().to_ascii_lowercase().as_str() {
        "win" | "windows" => "windows".to_string(),
        "mac" | "macos" | "darwin" => "macos".to_string(),
        "linux" => "linux".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn platform_matches(platforms: &[String]) -> bool {
    platforms.is_empty()
        || platforms
            .iter()
            .any(|platform| platform == current_platform())
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub(super) fn load_usage(skill_dir: &Path) -> Option<super::SkillUsage> {
    std::fs::read_to_string(skill_dir.join(super::USAGE_FILE_NAME))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

pub(super) fn save_usage(skill_dir: &Path, usage: &super::SkillUsage) -> Result<()> {
    std::fs::create_dir_all(skill_dir)?;
    let content = serde_json::to_string_pretty(usage).map_err(|error| {
        PureError::ConfigError(format!("failed to serialize skill usage: {error}"))
    })?;
    std::fs::write(skill_dir.join(super::USAGE_FILE_NAME), content)?;
    Ok(())
}

pub(super) fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

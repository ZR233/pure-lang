use std::path::{Component, Path, PathBuf};

use pl_protocol::Result;

use super::ops::tool_error;

pub(super) fn resolve_container_copy_path(path: &str, cwd: Option<&str>) -> Result<String> {
    if path.starts_with('/') {
        let normalized = normalize_container_path(Path::new(path))?;
        if let Some(cwd) = cwd.filter(|cwd| cwd.starts_with('/')) {
            let base = normalize_container_path(Path::new(cwd))?;
            if !normalized.starts_with(&base) {
                return Err(tool_error(
                    "file",
                    format!("path `{path}` escapes container cwd `{cwd}`"),
                ));
            }
        }
        return Ok(normalized.to_string_lossy().into_owned());
    }
    let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty() && *cwd != ".") else {
        return Ok(normalize_container_path(Path::new(path))?
            .to_string_lossy()
            .into_owned());
    };
    let base = normalize_container_path(Path::new(cwd))?;
    let normalized = normalize_container_path(&base.join(path))?;
    if cwd.starts_with('/') && !normalized.starts_with(&base) {
        return Err(tool_error(
            "file",
            format!("path `{path}` escapes container cwd `{cwd}`"),
        ));
    }
    Ok(normalized.to_string_lossy().into_owned())
}

fn normalize_container_path(path: &Path) -> Result<PathBuf> {
    let mut output = if path.is_absolute() {
        PathBuf::from("/")
    } else {
        PathBuf::new()
    };
    for component in path.components() {
        match component {
            Component::CurDir | Component::RootDir => {}
            Component::Normal(part) => output.push(part),
            Component::ParentDir => {
                if !output.pop() {
                    return Err(tool_error(
                        "file",
                        format!("path `{}` escapes container workspace", path.display()),
                    ));
                }
                if output.as_os_str().is_empty() && path.is_absolute() {
                    output.push("/");
                }
            }
            Component::Prefix(_) => {
                return Err(tool_error(
                    "file",
                    format!("unsupported container path `{}`", path.display()),
                ));
            }
        }
    }
    if output.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(output)
    }
}

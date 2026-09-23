use pl_protocol::Result;

use crate::tool_error;

pub(super) fn resolve_container_workspace_path(path: &str, cwd: Option<&str>) -> Result<String> {
    if path.starts_with('/') {
        return normalize_container_path(path);
    }
    let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty() && *cwd != ".") else {
        return normalize_container_path(path);
    };
    let base = normalize_container_path(cwd)?;
    let normalized = normalize_container_path(&format!("{base}/{path}"))?;
    if cwd.starts_with('/') && !is_within_container_path(&normalized, &base) {
        return Err(tool_error(
            "file",
            format!("path `{path}` escapes container cwd `{cwd}`"),
        ));
    }
    Ok(normalized)
}

fn normalize_container_path(path: &str) -> Result<String> {
    let absolute = path.starts_with('/');
    let mut components = Vec::new();

    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(tool_error(
                        "file",
                        format!("path `{path}` escapes container workspace"),
                    ));
                }
            }
            component => components.push(component),
        }
    }

    let normalized = components.join("/");
    if absolute {
        if normalized.is_empty() {
            Ok("/".to_string())
        } else {
            Ok(format!("/{normalized}"))
        }
    } else {
        Ok(if normalized.is_empty() {
            ".".to_string()
        } else {
            normalized
        })
    }
}

fn is_within_container_path(path: &str, base: &str) -> bool {
    base == "/"
        || path == base
        || path
            .strip_prefix(base)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

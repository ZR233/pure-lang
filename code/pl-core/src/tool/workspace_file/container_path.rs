use pl_protocol::Result;

use super::ops::tool_error;

pub(super) fn resolve_container_copy_path(path: &str, cwd: Option<&str>) -> Result<String> {
    if path.starts_with('/') {
<<<<<<< HEAD
        return normalize_container_path(path);
=======
        return Ok(normalize_container_path(Path::new(path))?
            .to_string_lossy()
            .into_owned());
>>>>>>> 6bd37cb0f58096be1872f15256a73a99e1a05ced
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_container_path_is_independent_from_cwd() {
        assert_eq!(
            resolve_container_copy_path(
                "/tmp/.mai-team/skills/demo/SKILL.md",
                Some("/workspace/repo"),
            )
            .expect("absolute container path"),
            "/tmp/.mai-team/skills/demo/SKILL.md",
        );
    }

    #[test]
    fn relative_container_path_uses_posix_separators_on_windows() {
        assert_eq!(
            resolve_container_copy_path("src/lib.rs", Some("/workspace/repo"))
                .expect("relative container path"),
            "/workspace/repo/src/lib.rs",
        );
    }

    #[test]
    fn relative_container_path_cannot_escape_absolute_cwd() {
        let error = resolve_container_copy_path("../secret", Some("/workspace/repo"))
            .expect_err("cwd escape must be rejected");

        assert!(error.to_string().contains("escapes container cwd"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_container_path_is_independent_from_cwd() {
        assert_eq!(
            resolve_container_copy_path(
                "/tmp/.mai-team/skills/demo/SKILL.md",
                Some("/workspace/repo"),
            )
            .expect("absolute container path"),
            "/tmp/.mai-team/skills/demo/SKILL.md",
        );
    }

    #[test]
    fn relative_container_path_is_resolved_from_cwd() {
        assert_eq!(
            resolve_container_copy_path("src/lib.rs", Some("/workspace/repo"))
                .expect("relative container path"),
            "/workspace/repo/src/lib.rs",
        );
    }

    #[test]
    fn relative_container_path_cannot_escape_absolute_cwd() {
        let error = resolve_container_copy_path("../secret", Some("/workspace/repo"))
            .expect_err("cwd escape must be rejected");

        assert!(error.to_string().contains("escapes container cwd"));
    }
}

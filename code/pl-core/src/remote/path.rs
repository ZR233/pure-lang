//! Host-independent POSIX path handling for remote workspaces.

use std::path::Path;

pub(super) fn relative_workspace_path(root: &str, path: &Path) -> Result<String, ()> {
    let root = normalize_root(root)?;
    let path = path.to_string_lossy().replace('\\', "/");
    let relative = if path.starts_with('/') {
        if path.trim_end_matches('/') == root {
            ""
        } else if root == "/" {
            path.trim_start_matches('/')
        } else {
            path.strip_prefix(&format!("{root}/")).ok_or(())?
        }
    } else {
        &path
    };
    normalize_relative(relative)
}

fn normalize_root(root: &str) -> Result<String, ()> {
    let root = root.replace('\\', "/");
    if !root.starts_with('/') {
        return Err(());
    }
    let components = normalized_components(&root)?;
    Ok(if components.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", components.join("/"))
    })
}

fn normalize_relative(path: &str) -> Result<String, ()> {
    let components = normalized_components(path)?;
    Ok(if components.is_empty() {
        ".".to_string()
    } else {
        components.join("/")
    })
}

fn normalized_components(path: &str) -> Result<Vec<&str>, ()> {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .map(|component| {
            if component == ".." {
                Err(())
            } else {
                Ok(component)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_paths_are_independent_from_host_path_semantics() {
        assert_eq!(
            relative_workspace_path("/srv/project", Path::new("/srv/project/src/lib.rs"))
                .expect("child"),
            "src/lib.rs"
        );
        assert_eq!(
            relative_workspace_path("/srv/project", Path::new(r"\srv\project\src\lib.rs"))
                .expect("backslash child"),
            "src/lib.rs"
        );
        assert!(relative_workspace_path("/srv/project", Path::new("/srv/project-other")).is_err());
        assert!(relative_workspace_path("/srv/project", Path::new("../outside")).is_err());
    }
}

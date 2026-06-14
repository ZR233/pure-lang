use std::path::{Component, Path, PathBuf};

use pl_protocol::PureError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathAccess {
    Workspace,
    External,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolPathPolicy {
    root_canonical: PathBuf,
    allow_workspace_escape: bool,
    tool: String,
}

impl ToolPathPolicy {
    pub(crate) fn new(
        root: PathBuf,
        allow_workspace_escape: bool,
        tool: impl Into<String>,
    ) -> Result<Self, PureError> {
        let tool = tool.into();
        let root_canonical =
            std::fs::canonicalize(&root).map_err(|error| PureError::ToolExecutionFailed {
                tool: tool.clone(),
                error: format!("failed to resolve workspace root: {error}"),
            })?;
        Ok(Self {
            root_canonical,
            allow_workspace_escape,
            tool,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root_canonical
    }

    pub(crate) fn resolve_existing(&self, path: &str) -> Result<PathBuf, PureError> {
        self.resolve_existing_path(Path::new(path), path)
    }

    pub(crate) fn resolve_existing_path(
        &self,
        path: &Path,
        original: &str,
    ) -> Result<PathBuf, PureError> {
        self.validate_for_execution(path, original)?;
        let candidate = self.candidate(path);
        let canonical = std::fs::canonicalize(&candidate)
            .map_err(|error| self.error(format!("failed to resolve path '{original}': {error}")))?;
        self.ensure_allowed(&canonical, original)?;
        Ok(canonical)
    }

    pub(crate) fn resolve_existing_directory(
        &self,
        path: &Path,
        original: &str,
    ) -> Result<PathBuf, PureError> {
        let resolved = self.resolve_existing_path(path, original)?;
        let metadata = std::fs::metadata(&resolved).map_err(|error| {
            self.error(format!(
                "failed to inspect directory '{}': {error}",
                resolved.display()
            ))
        })?;
        if !metadata.is_dir() {
            return Err(self.error(format!("path '{}' is not a directory", resolved.display())));
        }
        Ok(resolved)
    }

    pub(crate) fn resolve_for_write(&self, path: &str) -> Result<PathBuf, PureError> {
        self.resolve_existing_or_parent_path(Path::new(path), path)
    }

    pub(crate) fn resolve_existing_or_parent_path(
        &self,
        path: &Path,
        original: &str,
    ) -> Result<PathBuf, PureError> {
        self.validate_for_execution(path, original)?;
        let candidate = self.candidate(path);
        let (ancestor, tail) = existing_ancestor_and_tail(&candidate).map_err(|error| {
            self.error(format!(
                "failed to inspect parent for path '{original}': {error}"
            ))
        })?;
        let canonical = std::fs::canonicalize(&ancestor).map_err(|error| {
            self.error(format!(
                "failed to resolve parent for path '{original}': {error}"
            ))
        })?;
        self.ensure_allowed(&canonical, original)?;
        if tail.as_os_str().is_empty() {
            Ok(canonical)
        } else {
            Ok(canonical.join(tail))
        }
    }

    pub(crate) fn access_for_input(&self, path: &str) -> PathAccess {
        let path = Path::new(path.trim());
        if path.as_os_str().is_empty() {
            return PathAccess::Workspace;
        }
        if path_has_parent(path) || path_has_ambiguous_anchor(path) {
            return PathAccess::External;
        }
        let candidate = self.candidate(path);
        let resolved = existing_ancestor_and_tail(&candidate)
            .ok()
            .and_then(|(ancestor, tail)| {
                std::fs::canonicalize(ancestor).ok().map(|path| {
                    if tail.as_os_str().is_empty() {
                        path
                    } else {
                        path.join(tail)
                    }
                })
            })
            .unwrap_or(candidate);
        if path_is_inside_workspace(&resolved, &self.root_canonical) {
            PathAccess::Workspace
        } else {
            PathAccess::External
        }
    }

    pub(crate) fn display_relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root_canonical)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    fn validate_for_execution(&self, path: &Path, original: &str) -> Result<(), PureError> {
        if path_has_ambiguous_anchor(path) {
            return Err(self.error(format!(
                "path '{}' cannot be resolved without a current directory",
                path.display()
            )));
        }
        if self.allow_workspace_escape {
            return Ok(());
        }
        if path_has_parent(path) {
            return Err(self.error(format!("path '{original}' escapes the workspace")));
        }
        Ok(())
    }

    pub(crate) fn candidate(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root_canonical.join(path)
        }
    }

    fn ensure_allowed(&self, path: &Path, original: &str) -> Result<(), PureError> {
        if self.allow_workspace_escape || path_is_inside_workspace(path, &self.root_canonical) {
            return Ok(());
        }
        Err(self.error(format!("path '{original}' is outside the workspace")))
    }

    fn error(&self, error: String) -> PureError {
        PureError::ToolExecutionFailed {
            tool: self.tool.clone(),
            error,
        }
    }
}

fn existing_ancestor_and_tail(candidate: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
    let mut current = candidate.to_path_buf();
    loop {
        if current.exists() {
            let tail = candidate
                .strip_prefix(&current)
                .unwrap_or_else(|_| Path::new(""))
                .to_path_buf();
            return Ok((current, tail));
        }
        if !current.pop() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "path has no existing parent",
            ));
        }
    }
}

fn path_has_parent(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn path_has_ambiguous_anchor(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .any(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
}

pub(crate) fn path_is_inside_workspace(path: &Path, workspace_root: &Path) -> bool {
    #[cfg(windows)]
    {
        let path = comparable_windows_path(path);
        let root = comparable_windows_path(workspace_root);
        if path == root {
            return true;
        }
        let boundary = if root.ends_with('\\') {
            root
        } else {
            format!("{root}\\")
        };
        path.starts_with(&boundary)
    }
    #[cfg(not(windows))]
    {
        path.starts_with(workspace_root)
    }
}

#[cfg(windows)]
fn comparable_windows_path(path: &Path) -> String {
    let path = normalize_windows_verbatim_path(&path.to_string_lossy()).replace('/', "\\");
    path.trim_end_matches('\\').to_ascii_lowercase()
}

#[cfg(windows)]
fn normalize_windows_verbatim_path(path: &str) -> String {
    if let Some(path) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{path}")
    } else if let Some(path) = path.strip_prefix(r"\\?\") {
        path.to_string()
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pure-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn relative_path_resolves_from_workspace_root() {
        let workspace = unique_temp_dir("path-policy-relative");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        let file = workspace.join("src/lib.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let policy = ToolPathPolicy::new(workspace.clone(), false, "test").unwrap();

        let resolved = policy.resolve_existing("src/lib.rs").unwrap();

        assert_eq!(resolved, std::fs::canonicalize(&file).unwrap());
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn write_target_allows_missing_child_when_parent_resolves() {
        let workspace = unique_temp_dir("path-policy-write");
        std::fs::create_dir_all(&workspace).unwrap();
        let policy = ToolPathPolicy::new(workspace.clone(), false, "test").unwrap();

        let resolved = policy.resolve_for_write("new/child.txt").unwrap();

        assert_eq!(
            resolved,
            std::fs::canonicalize(&workspace)
                .unwrap()
                .join("new/child.txt")
        );
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn workspace_only_rejects_parent_segments() {
        let workspace = unique_temp_dir("path-policy-parent");
        std::fs::create_dir_all(&workspace).unwrap();
        let policy = ToolPathPolicy::new(workspace.clone(), false, "test").unwrap();

        let result = policy.resolve_for_write("../outside.txt");

        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn access_classification_detects_external_absolute_path() {
        let workspace = unique_temp_dir("path-policy-access-workspace");
        let outside = unique_temp_dir("path-policy-access-outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let policy = ToolPathPolicy::new(workspace.clone(), false, "test").unwrap();

        assert_eq!(
            policy.access_for_input(outside.to_str().unwrap()),
            PathAccess::External
        );

        let _ = std::fs::remove_dir_all(workspace);
        let _ = std::fs::remove_dir_all(outside);
    }
}

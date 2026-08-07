use std::path::{Component, Path, PathBuf};

use pl_protocol::PureError;

use crate::path_safety::{
    PathSafetyError, is_lexically_within, validate_existing_path, validate_path_for_write,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAccess {
    Workspace,
    External,
}

#[derive(Debug, Clone)]
pub struct ToolPathPolicy {
    root_canonical: PathBuf,
    allow_workspace_escape: bool,
    tool: String,
}

impl ToolPathPolicy {
    pub fn new(
        root: PathBuf,
        allow_workspace_escape: bool,
        tool: impl Into<String>,
    ) -> Result<Self, PureError> {
        let tool = tool.into();
        let root_canonical =
            dunce::canonicalize(&root).map_err(|error| PureError::ToolExecutionFailed {
                tool: tool.clone(),
                error: format!("failed to resolve workspace root: {error}"),
            })?;
        Ok(Self {
            root_canonical,
            allow_workspace_escape,
            tool,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root_canonical
    }

    pub fn resolve_existing(&self, path: &str) -> Result<PathBuf, PureError> {
        self.resolve_existing_path(Path::new(path), path)
    }

    pub fn resolve_existing_path(&self, path: &Path, original: &str) -> Result<PathBuf, PureError> {
        self.validate_for_execution(path, original)?;
        let candidate = lexical_normalize(&self.candidate(path));
        let safety_root = self.safety_root(&candidate)?;
        validate_existing_path(&safety_root, &candidate).map_err(|error| match error {
            PathSafetyError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                self.error(format!("failed to resolve path '{original}': {source}"))
            }
            error => self.error(error.to_string()),
        })?;
        let canonical = dunce::canonicalize(&candidate)
            .map_err(|error| self.error(format!("failed to resolve path '{original}': {error}")))?;
        self.ensure_allowed(&canonical, original)?;
        Ok(canonical)
    }

    pub fn resolve_existing_directory(
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

    pub fn resolve_for_write(&self, path: &str) -> Result<PathBuf, PureError> {
        self.resolve_existing_or_parent(path)
    }

    pub fn resolve_existing_or_parent(&self, path: &str) -> Result<PathBuf, PureError> {
        self.resolve_existing_or_parent_path(Path::new(path), path)
    }

    pub fn resolve_existing_or_parent_path(
        &self,
        path: &Path,
        original: &str,
    ) -> Result<PathBuf, PureError> {
        self.validate_for_execution(path, original)?;
        let candidate = lexical_normalize(&self.candidate(path));
        let safety_root = self.safety_root(&candidate)?;
        validate_path_for_write(&safety_root, &candidate)
            .map_err(|error| self.error(error.to_string()))?;
        let (ancestor, tail) = existing_ancestor_and_tail(&candidate).map_err(|error| {
            self.error(format!(
                "failed to inspect parent for path '{original}': {error}"
            ))
        })?;
        let canonical = dunce::canonicalize(&ancestor).map_err(|error| {
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

    fn safety_root(&self, candidate: &Path) -> Result<PathBuf, PureError> {
        if !self.allow_workspace_escape || is_lexically_within(&self.root_canonical, candidate) {
            return Ok(self.root_canonical.clone());
        }
        absolute_path_anchor(candidate).ok_or_else(|| {
            self.error(format!(
                "path '{}' has no absolute filesystem anchor",
                candidate.display()
            ))
        })
    }

    pub fn access_for_input(&self, path: &str) -> PathAccess {
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
                dunce::canonicalize(ancestor).ok().map(|path| {
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

    pub fn display_relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root_canonical)
            .unwrap_or(path)
            .display()
            .to_string()
            .replace(std::path::MAIN_SEPARATOR, "/")
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
        match std::fs::symlink_metadata(&current) {
            Ok(_) => {
                let tail = candidate
                    .strip_prefix(&current)
                    .unwrap_or_else(|_| Path::new(""))
                    .to_path_buf();
                return Ok((current, tail));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if !current.pop() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "path has no existing parent",
            ));
        }
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn absolute_path_anchor(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut anchor = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => anchor.push(prefix.as_os_str()),
            Component::RootDir => anchor.push(component.as_os_str()),
            Component::CurDir | Component::ParentDir | Component::Normal(_) => break,
        }
    }
    (!anchor.as_os_str().is_empty()).then_some(anchor)
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

        assert_eq!(resolved, dunce::canonicalize(&file).unwrap());
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
            dunce::canonicalize(&workspace)
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

    #[test]
    fn existing_and_write_paths_reject_link_ancestors() {
        let workspace = unique_temp_dir("path-policy-link-workspace");
        let outside = unique_temp_dir("path-policy-link-outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("existing.txt"), "outside").unwrap();
        create_directory_link(&outside, &workspace.join("linked"));
        let policy = ToolPathPolicy::new(workspace.clone(), false, "test").unwrap();

        let existing = policy.resolve_existing("linked/existing.txt").unwrap_err();
        let write = policy.resolve_for_write("linked/new.txt").unwrap_err();

        assert!(existing.to_string().contains("reparse point"));
        assert!(write.to_string().contains("reparse point"));
        remove_directory_link(&workspace.join("linked"));
        std::fs::remove_dir_all(workspace).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn absolute_drive_path_resolves_to_native_non_verbatim_workspace_root() {
        let workspace = unique_temp_dir("path-policy-verbatim-workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let file = workspace.join("file.txt");
        std::fs::write(&file, "content").unwrap();
        let policy = ToolPathPolicy::new(workspace.clone(), false, "test").unwrap();

        assert_eq!(
            policy
                .resolve_existing_path(&file, &file.to_string_lossy())
                .unwrap(),
            dunce::canonicalize(&file).unwrap()
        );

        assert!(!policy.root().to_string_lossy().starts_with(r"\\?\"));
        assert!(
            !policy
                .resolve_existing_path(&file, &file.to_string_lossy())
                .unwrap()
                .to_string_lossy()
                .starts_with(r"\\?\")
        );

        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) {
        std::os::windows::fs::symlink_dir(target, link).unwrap();
    }

    #[cfg(unix)]
    fn remove_directory_link(link: &Path) {
        std::fs::remove_file(link).unwrap();
    }

    #[cfg(windows)]
    fn remove_directory_link(link: &Path) {
        std::fs::remove_dir(link).unwrap();
    }
}

use std::path::{Component, Path, PathBuf};

use pl_protocol::PureError;

#[derive(Debug, Clone)]
pub struct WorkspacePaths {
    root_canonical: PathBuf,
}

impl WorkspacePaths {
    pub async fn new(root: PathBuf) -> Result<Self, PureError> {
        let root_canonical = tokio::fs::canonicalize(&root).await.map_err(|error| {
            PureError::ToolExecutionFailed {
                tool: "file".to_string(),
                error: format!("failed to resolve workspace root: {error}"),
            }
        })?;
        Ok(Self { root_canonical })
    }

    pub async fn resolve_existing(&self, path: &str) -> Result<PathBuf, PureError> {
        self.reject_parent_components(path)?;
        let candidate = self.candidate(path);
        let canonical = tokio::fs::canonicalize(&candidate)
            .await
            .map_err(|error| self.error(format!("failed to resolve path '{path}': {error}")))?;
        self.ensure_inside_workspace(&canonical, path)?;
        Ok(canonical)
    }

    pub async fn resolve_for_write(&self, path: &str) -> Result<PathBuf, PureError> {
        self.reject_parent_components(path)?;
        let candidate = self.candidate(path);
        let mut ancestor = candidate
            .parent()
            .ok_or_else(|| self.error(format!("path '{path}' has no parent directory")))?
            .to_path_buf();
        while !tokio::fs::try_exists(&ancestor).await.map_err(|error| {
            self.error(format!(
                "failed to inspect parent for path '{path}': {error}"
            ))
        })? {
            if !ancestor.pop() {
                return Err(self.error(format!("path '{path}' has no existing parent")));
            }
        }
        self.resolve_existing_path(&ancestor, path).await?;
        Ok(candidate)
    }

    pub async fn reject_symlink_write(&self, path: &Path) -> Result<(), PureError> {
        match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(self.error(format!(
                "refusing to write through symbolic link '{}'",
                path.display()
            ))),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(self.error(format!("failed to inspect '{}': {error}", path.display())))
            }
        }
    }

    pub fn display_relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root_canonical)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    fn candidate(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            self.root_canonical.join(path)
        }
    }

    async fn resolve_existing_path(
        &self,
        path: &Path,
        original: &str,
    ) -> Result<PathBuf, PureError> {
        let canonical = tokio::fs::canonicalize(path).await.map_err(|error| {
            self.error(format!(
                "failed to resolve parent for path '{original}': {error}"
            ))
        })?;
        self.ensure_inside_workspace(&canonical, original)?;
        Ok(canonical)
    }

    fn reject_parent_components(&self, path: &str) -> Result<(), PureError> {
        let path = Path::new(path);
        if path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            ) && !path.is_absolute()
        }) {
            return Err(self.error(format!("path '{}' escapes the workspace", path.display())));
        }
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(self.error(format!("path '{}' escapes the workspace", path.display())));
        }
        Ok(())
    }

    fn ensure_inside_workspace(&self, path: &Path, original: &str) -> Result<(), PureError> {
        if path.starts_with(&self.root_canonical) {
            return Ok(());
        }
        Err(self.error(format!("path '{original}' is outside the workspace")))
    }

    fn error(&self, error: String) -> PureError {
        PureError::ToolExecutionFailed {
            tool: "file".to_string(),
            error,
        }
    }
}

pub fn matches_pattern(path: &str, pattern: Option<&str>) -> bool {
    let Some(pattern) = pattern.filter(|pattern| !pattern.is_empty()) else {
        return true;
    };
    if !pattern.contains('*') {
        return path.contains(pattern);
    }

    let mut remainder = path;
    let mut first = true;
    for part in pattern.split('*').filter(|part| !part.is_empty()) {
        if first && !pattern.starts_with('*') {
            let Some(stripped) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = stripped;
        } else {
            let Some(index) = remainder.find(part) else {
                return false;
            };
            remainder = &remainder[index + part.len()..];
        }
        first = false;
    }

    pattern.ends_with('*') || remainder.is_empty()
}

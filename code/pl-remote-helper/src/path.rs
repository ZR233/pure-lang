use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};

use pl_protocol::remote::{RemoteError, RemoteErrorCode};

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceRegistry {
    roots: HashMap<String, PathBuf>,
    next_id: u64,
}

impl WorkspaceRegistry {
    pub(crate) async fn resolve_workspace_root(path: &str) -> Result<PathBuf, RemoteError> {
        let canonical = tokio::fs::canonicalize(path)
            .await
            .map_err(|error| io_error("failed to open workspace", error))?;
        let metadata = tokio::fs::metadata(&canonical)
            .await
            .map_err(|error| io_error("failed to inspect workspace", error))?;
        if !metadata.is_dir() {
            return Err(remote_error(
                RemoteErrorCode::InvalidRequest,
                "workspace path is not a directory",
            ));
        }
        Ok(canonical)
    }

    pub(crate) fn open_resolved(&mut self, canonical: PathBuf) -> (String, PathBuf) {
        self.next_id = self.next_id.saturating_add(1);
        let id = format!("workspace-{}", self.next_id);
        self.roots.insert(id.clone(), canonical.clone());
        (id, canonical)
    }

    pub(crate) fn close(&mut self, workspace_id: &str) -> Result<(), RemoteError> {
        self.roots.remove(workspace_id).map(|_| ()).ok_or_else(|| {
            remote_error(
                RemoteErrorCode::WorkspaceNotFound,
                format!("unknown workspace id '{workspace_id}'"),
            )
        })
    }

    pub(crate) async fn resolve_existing(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<PathBuf, RemoteError> {
        let (root, joined) = self.join(workspace_id, path)?;
        reject_symlink_ancestors(root, &joined, false).await?;
        tokio::fs::canonicalize(&joined)
            .await
            .map_err(|error| io_error("failed to resolve remote path", error))
            .and_then(|canonical| ensure_within(root, canonical))
    }

    pub(crate) async fn resolve_for_write(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<PathBuf, RemoteError> {
        let (root, joined) = self.join(workspace_id, path)?;
        reject_symlink_ancestors(root, &joined, true).await?;
        ensure_within(root, joined)
    }

    pub(crate) fn root(&self, workspace_id: &str) -> Result<&Path, RemoteError> {
        self.roots
            .get(workspace_id)
            .map(PathBuf::as_path)
            .ok_or_else(|| {
                remote_error(
                    RemoteErrorCode::WorkspaceNotFound,
                    format!("unknown workspace id '{workspace_id}'"),
                )
            })
    }

    fn join<'a>(
        &'a self,
        workspace_id: &str,
        path: &str,
    ) -> Result<(&'a Path, PathBuf), RemoteError> {
        let root = self.root(workspace_id)?;
        let relative = normalize_relative(path)?;
        Ok((root, root.join(relative)))
    }
}

fn normalize_relative(path: &str) -> Result<PathBuf, RemoteError> {
    let mut normalized = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(remote_error(
                    RemoteErrorCode::WorkspaceEscape,
                    format!("path '{path}' escapes the workspace"),
                ));
            }
        }
    }
    Ok(normalized)
}

async fn reject_symlink_ancestors(
    root: &Path,
    path: &Path,
    allow_missing_tail: bool,
) -> Result<(), RemoteError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        remote_error(
            RemoteErrorCode::WorkspaceEscape,
            "path escapes the workspace",
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match tokio::fs::symlink_metadata(&current).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(remote_error(
                    RemoteErrorCode::WorkspaceEscape,
                    format!("symbolic link entry '{}' is not allowed", current.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound && allow_missing_tail => break,
            Err(error) => return Err(io_error("failed to inspect remote path", error)),
        }
    }
    Ok(())
}

fn ensure_within(root: &Path, path: PathBuf) -> Result<PathBuf, RemoteError> {
    if path == root || path.starts_with(root) {
        Ok(path)
    } else {
        Err(remote_error(
            RemoteErrorCode::WorkspaceEscape,
            "path escapes the workspace",
        ))
    }
}

pub(crate) fn remote_error(code: RemoteErrorCode, message: impl Into<String>) -> RemoteError {
    RemoteError {
        code,
        message: message.into(),
    }
}

pub(crate) fn io_error(operation: &str, error: io::Error) -> RemoteError {
    let code = if error.kind() == io::ErrorKind::NotFound {
        RemoteErrorCode::PathNotFound
    } else {
        RemoteErrorCode::Io
    };
    remote_error(code, format!("{operation}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn workspace_rejects_parent_and_symlink_escape() {
        let temp = tempfile::tempdir().expect("tempdir");
        #[cfg(unix)]
        let outside = tempfile::tempdir().expect("outside");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).expect("symlink");
        let mut registry = WorkspaceRegistry::default();
        let root =
            WorkspaceRegistry::resolve_workspace_root(temp.path().to_str().expect("utf8 path"))
                .await
                .expect("open workspace");
        let (id, _) = registry.open_resolved(root);

        assert!(registry.resolve_existing(&id, "../secret").await.is_err());
        #[cfg(unix)]
        assert!(registry.resolve_existing(&id, "link").await.is_err());
    }
}

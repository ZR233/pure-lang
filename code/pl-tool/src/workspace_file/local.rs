use std::path::{Path, PathBuf};

use pl_protocol::Result;
use tokio::io::AsyncReadExt;

use crate::file::path::{WorkspacePaths, matches_pattern};
use crate::workspace::ToolWorkspace;
use crate::workspace::path_safety::{metadata_if_real_async, real_directory_entries_async};

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadBytesRequest, WorkspaceFileReadRequest, WorkspaceFileRemoveRequest,
    WorkspaceFileStat, WorkspaceFileStatRequest, WorkspaceFileWriteRequest,
};
use crate::tool_error;

#[derive(Debug, Clone)]
pub struct LocalWorkspaceFileBackend {
    paths: WorkspacePaths,
    lsp_runtime: Option<pl_lsp::runtime::LspRuntimeRegistry>,
    workspace: Option<ToolWorkspace>,
}

impl LocalWorkspaceFileBackend {
    pub async fn new(root: PathBuf, allow_workspace_escape: bool) -> Result<Self> {
        Ok(Self {
            paths: WorkspacePaths::new(root, allow_workspace_escape).await?,
            lsp_runtime: None,
            workspace: None,
        })
    }

    /// Binds a confined file backend to the host's frozen write policy and LSP lease.
    ///
    /// # Errors
    /// Rejects an unavailable workspace root before any file operation is admitted.
    pub async fn confined(workspace: ToolWorkspace) -> Result<Self> {
        let mut backend = Self::new(workspace.root().to_path_buf(), false).await?;
        backend.lsp_runtime = workspace.lsp_runtime();
        backend.workspace = Some(workspace);
        Ok(backend)
    }

    fn with_cwd(&self, cwd: Option<&str>, path: &str) -> Result<String> {
        let path_ref = Path::new(path);
        if path_ref.is_absolute() {
            return Ok(path.to_string());
        }
        let Some(cwd) = cwd.filter(|cwd| !cwd.trim().is_empty() && *cwd != ".") else {
            return Ok(path.to_string());
        };
        let cwd_ref = Path::new(cwd);
        if self.paths.allows_host_access() {
            return Ok(cwd_ref.join(path_ref).to_string_lossy().into_owned());
        }
        if cwd_ref.is_absolute() {
            return Err(tool_error(
                "file",
                "cwd must be a workspace-relative path for local file tools",
            ));
        }
        let mut joined = PathBuf::new();
        for component in cwd_ref.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::Normal(part) => joined.push(part),
                std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_) => {
                    return Err(tool_error(
                        "file",
                        "cwd must not escape the workspace for local file tools",
                    ));
                }
            }
        }
        joined.push(path_ref);
        Ok(joined.to_string_lossy().into_owned())
    }

    async fn resolve_existing(&self, cwd: Option<&str>, path: &str) -> Result<PathBuf> {
        let path = self.with_cwd(cwd, path)?;
        self.paths.resolve_existing(&path).await
    }

    async fn resolve_for_write(&self, cwd: Option<&str>, path: &str) -> Result<PathBuf> {
        let path = self.with_cwd(cwd, path)?;
        self.paths.resolve_for_write(&path).await
    }

    async fn resolve_existing_or_parent(&self, cwd: Option<&str>, path: &str) -> Result<PathBuf> {
        let path = self.with_cwd(cwd, path)?;
        self.paths.resolve_existing_or_parent(&path).await
    }

    async fn notify_changed(&self, path: &Path) {
        if let Some(registry) = &self.lsp_runtime {
            registry.notify_file_changed(path.to_path_buf()).await;
        }
    }

    async fn notify_deleted(&self, path: &Path) {
        if let Some(registry) = &self.lsp_runtime {
            registry.notify_file_deleted(path.to_path_buf()).await;
        }
    }
}

impl WorkspaceFileBackend for LocalWorkspaceFileBackend {
    fn for_grant(&self, grant: &pl_core::tool::execution_policy::ExecutionGrant) -> Option<Self> {
        if !grant.contains(crate::approval::HOST_WORKSPACE_ACCESS)
            || !self
                .workspace
                .as_ref()
                .is_some_and(|workspace| workspace.workspace().boundary().allows_host_paths())
        {
            return None;
        }
        let mut backend = self.clone();
        backend.paths = self.paths.with_host_access();
        Some(backend)
    }

    async fn default_cwd(&self) -> Result<String> {
        Ok(".".to_string())
    }

    async fn stat_optional(
        &self,
        request: WorkspaceFileStatRequest,
    ) -> Result<Option<WorkspaceFileStat>> {
        let path = self
            .resolve_existing_or_parent(request.cwd.as_deref(), &request.path)
            .await?;
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        Ok(Some(WorkspaceFileStat {
            path: self.paths.display_relative(&path),
            is_file: metadata.is_file(),
            is_dir: metadata.is_dir(),
            len: metadata.is_file().then_some(metadata.len()),
            readonly: Some(metadata.permissions().readonly()),
            modified_at: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|duration| i64::try_from(duration.as_secs()).ok()),
        }))
    }

    async fn read_text(&self, request: WorkspaceFileReadRequest) -> Result<String> {
        let input_path = self.with_cwd(request.cwd.as_deref(), &request.path)?;
        let path = self.paths.resolve_existing(&input_path).await?;
        let metadata = tokio::fs::metadata(&path).await?;
        if !metadata.is_file() {
            return Err(tool_error(
                "read_file",
                format!("'{}' is not a regular file", request.path),
            ));
        }
        tokio::fs::read_to_string(&path).await.map_err(|error| {
            tool_error(
                "read_file",
                format!(
                    "failed to read '{}': {error}",
                    self.paths.display_relative(&path)
                ),
            )
        })
    }

    async fn read_bytes(&self, request: WorkspaceFileReadBytesRequest) -> Result<Vec<u8>> {
        let input_path = self.with_cwd(request.cwd.as_deref(), &request.path)?;
        let path = self.paths.resolve_existing(&input_path).await?;
        let metadata = tokio::fs::metadata(&path).await?;
        if !metadata.is_file() {
            return Err(tool_error(
                "view_image",
                format!("'{}' is not a regular file", request.path),
            ));
        }
        if metadata.len() > request.max_bytes as u64 {
            return Err(tool_error(
                "view_image",
                format!(
                    "'{}' exceeds the {} byte source limit",
                    self.paths.display_relative(&path),
                    request.max_bytes
                ),
            ));
        }
        let file = tokio::fs::File::open(&path).await.map_err(|error| {
            tool_error(
                "view_image",
                format!(
                    "failed to read '{}': {error}",
                    self.paths.display_relative(&path)
                ),
            )
        })?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(request.max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| {
                tool_error(
                    "view_image",
                    format!(
                        "failed to read '{}': {error}",
                        self.paths.display_relative(&path)
                    ),
                )
            })?;
        if bytes.len() > request.max_bytes {
            return Err(tool_error(
                "view_image",
                format!(
                    "'{}' changed while reading and exceeds the {} byte source limit",
                    self.paths.display_relative(&path),
                    request.max_bytes
                ),
            ));
        }
        Ok(bytes)
    }

    async fn write_text(&self, request: WorkspaceFileWriteRequest) -> Result<()> {
        let path = self
            .resolve_for_write(request.cwd.as_deref(), &request.path)
            .await?;
        if let Some(workspace) = &self.workspace {
            workspace.ensure_path_writable(&path)?;
        }
        let target = path.clone();
        tokio::task::spawn_blocking(move || {
            crate::workspace::write_file_with_mode(
                &target,
                request.content.as_bytes(),
                request.mode,
            )
        })
        .await
        .map_err(|error| tool_error("write_file", error))??;
        self.notify_changed(&path).await;
        Ok(())
    }

    async fn remove_file(&self, request: WorkspaceFileRemoveRequest) -> Result<()> {
        let path = self
            .resolve_existing(request.cwd.as_deref(), &request.path)
            .await?;
        if let Some(workspace) = &self.workspace {
            workspace.ensure_path_writable(&path)?;
        }
        let metadata = tokio::fs::metadata(&path).await?;
        if !metadata.is_file() {
            return Err(tool_error(
                "apply_patch",
                format!("cannot delete '{}': path is not a file", request.path),
            ));
        }
        tokio::fs::remove_file(&path).await.map_err(|error| {
            tool_error(
                "apply_patch",
                format!(
                    "failed to delete '{}': {error}",
                    self.paths.display_relative(&path)
                ),
            )
        })?;
        self.notify_deleted(&path).await;
        Ok(())
    }

    async fn list(&self, request: WorkspaceFileListRequest) -> Result<WorkspaceFileListResult> {
        let root = self
            .resolve_existing_or_parent(request.cwd.as_deref(), &request.path)
            .await?;
        if !tokio::fs::try_exists(&root).await? {
            return Ok(WorkspaceFileListResult {
                files: Vec::new(),
                truncated: false,
            });
        }
        let mut files = Vec::new();
        collect_entries(
            &self.paths,
            &root,
            &request.glob,
            request.include_dirs,
            request.max_files.saturating_add(1),
            &mut files,
        )
        .await?;
        files.sort();
        let truncated = files.len() > request.max_files;
        files.truncate(request.max_files);
        Ok(WorkspaceFileListResult { files, truncated })
    }
}

async fn collect_entries(
    paths: &WorkspacePaths,
    root: &Path,
    glob: &str,
    include_dirs: bool,
    limit: usize,
    output: &mut Vec<String>,
) -> Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        if output.len() >= limit {
            break;
        }
        let Some(metadata) = traversal_metadata(&path).await? else {
            continue;
        };
        if metadata.is_dir() {
            if is_skipped_dir(&path) {
                continue;
            }
            let display = paths.display_relative(&path);
            if include_dirs && path != root && matches_list_entry(root, &path, &display, glob, true)
            {
                output.push(format!("{display}/"));
            }
            for entry in real_directory_entries_async(&path)
                .await
                .map_err(|error| tool_error("file", error))?
            {
                stack.push(entry);
            }
        } else if metadata.is_file() {
            let display = paths.display_relative(&path);
            if matches_list_entry(root, &path, &display, glob, false) {
                output.push(display);
            }
        }
    }
    Ok(())
}

fn matches_list_entry(root: &Path, path: &Path, display: &str, glob: &str, is_dir: bool) -> bool {
    if matches_entry_candidate(display, glob) {
        return true;
    }
    if is_dir && path != root && matches_entry_candidate(&format!("{display}/"), glob) {
        return true;
    }
    let Some(path_relative) = display_relative_to(root, path) else {
        return false;
    };
    if path_relative.is_empty() {
        return false;
    }
    matches_entry_candidate(&path_relative, glob)
        || (is_dir && matches_entry_candidate(&format!("{path_relative}/"), glob))
}

fn matches_entry_candidate(candidate: &str, glob: &str) -> bool {
    matches_pattern(candidate, Some(glob))
}

fn display_relative_to(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return Some(String::new());
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

async fn traversal_metadata(path: &Path) -> Result<Option<std::fs::Metadata>> {
    metadata_if_real_async(path)
        .await
        .map_err(|error| tool_error("file", error))
}

fn is_skipped_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{AgentWorkspace, WorkspaceMutability, WriteMode};
    use pl_core::tool::execution_policy::ExecutionGrant;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn invocation_grant_does_not_mutate_default_scope_or_bypass_directory_write_policy() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let external = outside.path().join("data.txt");
        tokio::fs::write(&external, "  外部文件\r\n").await.unwrap();
        let backend = LocalWorkspaceFileBackend::confined(ToolWorkspace::new(
            AgentWorkspace::directory(root.path(), Some(Vec::new())),
        ))
        .await
        .unwrap();
        let read = || WorkspaceFileReadRequest {
            path: external.to_string_lossy().into_owned(),
            cwd: None,
        };
        assert!(backend.read_text(read()).await.is_err());
        let grant =
            ExecutionGrant::default().with_capability(crate::approval::HOST_WORKSPACE_ACCESS);
        let permitted = backend.for_grant(&grant).unwrap();
        assert_eq!(permitted.read_text(read()).await.unwrap(), "  外部文件\r\n");
        assert!(
            permitted
                .write_text(WorkspaceFileWriteRequest {
                    mode: WriteMode::Create,
                    path: "forbidden.txt".into(),
                    cwd: None,
                    content: "not allowed".into(),
                })
                .await
                .is_err()
        );
        assert!(!root.path().join("forbidden.txt").exists());
        assert!(
            backend.read_text(read()).await.is_err(),
            "the next invocation must still require its own grant"
        );
        let confined = LocalWorkspaceFileBackend::confined(ToolWorkspace::new(
            AgentWorkspace::confined(root.path(), WorkspaceMutability::ReadWrite),
        ))
        .await
        .unwrap();
        assert!(
            confined.for_grant(&grant).is_none(),
            "physical confinement cannot be approved away"
        );
    }
}

use std::path::{Path, PathBuf};

use pl_protocol::Result;

use crate::path_safety::{metadata_if_real_async, real_directory_entries_async};
use crate::tool::ToolContext;
use crate::tool::file::path::{WorkspacePaths, matches_pattern};

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadRequest, WorkspaceFileRemoveRequest, WorkspaceFileSearchMatch,
    WorkspaceFileSearchRequest, WorkspaceFileSearchResult, WorkspaceFileStat,
    WorkspaceFileStatRequest, WorkspaceFileWriteRequest,
};
use super::ops::tool_error;

#[derive(Debug, Clone)]
pub struct LocalWorkspaceFileBackend {
    paths: WorkspacePaths,
    lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
}

impl LocalWorkspaceFileBackend {
    pub async fn new(root: PathBuf, allow_workspace_escape: bool) -> Result<Self> {
        Ok(Self {
            paths: WorkspacePaths::new(root, allow_workspace_escape).await?,
            lsp_runtime: None,
        })
    }

    pub async fn from_context(context: &ToolContext) -> Result<Self> {
        let mut backend = Self::new(
            context.workspace_root.clone(),
            context.allows_workspace_escape(),
        )
        .await?;
        backend.lsp_runtime = context.lsp_runtime.clone();
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
    async fn default_cwd(&self) -> Result<String> {
        Ok(".".to_string())
    }

    async fn stat(&self, request: WorkspaceFileStatRequest) -> Result<WorkspaceFileStat> {
        let path = self
            .resolve_existing(request.cwd.as_deref(), &request.path)
            .await?;
        let metadata = tokio::fs::metadata(&path).await?;
        Ok(WorkspaceFileStat {
            path: self.paths.display_relative(&path),
            is_file: metadata.is_file(),
            is_dir: metadata.is_dir(),
            len: metadata.is_file().then_some(metadata.len()),
        })
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

    async fn write_text(&self, request: WorkspaceFileWriteRequest) -> Result<()> {
        let path = self
            .resolve_for_write(request.cwd.as_deref(), &request.path)
            .await?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, request.content)
            .await
            .map_err(|error| {
                tool_error(
                    "apply_patch",
                    format!(
                        "failed to write '{}': {error}",
                        self.paths.display_relative(&path)
                    ),
                )
            })?;
        self.notify_changed(&path).await;
        Ok(())
    }

    async fn remove_file(&self, request: WorkspaceFileRemoveRequest) -> Result<()> {
        let path = self
            .resolve_existing(request.cwd.as_deref(), &request.path)
            .await?;
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

    async fn search(
        &self,
        request: WorkspaceFileSearchRequest,
    ) -> Result<WorkspaceFileSearchResult> {
        let root = self
            .resolve_existing(request.cwd.as_deref(), &request.path)
            .await?;
        let mut matches = Vec::new();
        search_entries(&self.paths, &root, &request, &mut matches).await?;
        let truncated = matches.len() > request.max_matches;
        matches.truncate(request.max_matches);
        Ok(WorkspaceFileSearchResult { matches, truncated })
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
            if include_dirs && matches_list_entry(root, &path, &display, glob, true) {
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

async fn search_entries(
    paths: &WorkspacePaths,
    root: &Path,
    request: &WorkspaceFileSearchRequest,
    output: &mut Vec<WorkspaceFileSearchMatch>,
) -> Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        if output.len() > request.max_matches {
            break;
        }
        let Some(metadata) = traversal_metadata(&path).await? else {
            continue;
        };
        if metadata.is_dir() {
            if is_skipped_dir(&path) {
                continue;
            }
            for entry in real_directory_entries_async(&path)
                .await
                .map_err(|error| tool_error("file", error))?
            {
                stack.push(entry);
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let display = paths.display_relative(&path);
        if !matches_pattern(&display, request.glob.as_deref()) {
            continue;
        }
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        for (line_index, line) in content.lines().enumerate() {
            if output.len() > request.max_matches {
                break;
            }
            if let Some(column) = match_line(line, &request.query, request.case_sensitive) {
                output.push(WorkspaceFileSearchMatch {
                    path: display.clone(),
                    line: line_index + 1,
                    column,
                    text: line.to_string(),
                });
            }
        }
    }
    Ok(())
}

async fn traversal_metadata(path: &Path) -> Result<Option<std::fs::Metadata>> {
    metadata_if_real_async(path)
        .await
        .map_err(|error| tool_error("file", error))
}

fn match_line(line: &str, query: &str, case_sensitive: bool) -> Option<usize> {
    if case_sensitive {
        line.find(query).map(|index| index + 1)
    } else {
        line.to_lowercase()
            .find(&query.to_lowercase())
            .map(|index| index + 1)
    }
}

fn is_skipped_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules"))
}

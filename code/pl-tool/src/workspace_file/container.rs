use crate::tool_error;
use std::sync::Arc;

use crate::container::helpers::preview_error;
use crate::container::{
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecRequest,
};
use crate::shell::shell_quote_word;

use pl_protocol::Result;

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadBytesRequest, WorkspaceFileReadRequest, WorkspaceFileRemoveRequest,
    WorkspaceFileStat, WorkspaceFileStatRequest, WorkspaceFileWriteRequest,
};
use super::container_path::resolve_container_workspace_path;

#[derive(Debug, Clone)]
pub struct ContainerWorkspaceFileBackend<B> {
    backend: Arc<B>,
}

impl<B> ContainerWorkspaceFileBackend<B> {
    pub fn new(backend: Arc<B>) -> Self {
        Self { backend }
    }
}

impl<B> WorkspaceFileBackend for ContainerWorkspaceFileBackend<B>
where
    B: ContainerBackend,
{
    async fn default_cwd(&self) -> Result<String> {
        let output = self
            .backend
            .exec(ContainerExecRequest {
                call_id: None,
                command:
                    "if [ -d /workspace/repo ]; then printf /workspace/repo; else printf /workspace; fi"
                        .to_string(),
                cwd: Some("/".to_string()),
                timeout_secs: Some(10),
                output_bytes_cap: None,
                cancellation_token: None,
            })
            .await
            .map_err(|error| tool_error("file", error))?;
        if output.status != 0 {
            return Err(tool_error(
                "file",
                format!(
                    "failed to resolve default cwd: {}",
                    preview_error(&output.stderr, &output.stdout)
                ),
            ));
        }
        Ok(output.stdout.trim().to_string())
    }

    async fn stat_optional(
        &self,
        request: WorkspaceFileStatRequest,
    ) -> Result<Option<WorkspaceFileStat>> {
        let path = resolve_container_workspace_path(&request.path, request.cwd.as_deref())?;
        let command = format!(
            "p={path}; if test -f \"$p\"; then printf 'file\t'; wc -c < \"$p\"; elif test -d \"$p\"; then printf 'dir\t0'; elif test -e \"$p\" || test -L \"$p\"; then printf 'other\t0'; else parent=\"$p\"; while test \"$parent\" != /; do parent=$(dirname -- \"$parent\") || exit 14; if test -d \"$parent\"; then test -x \"$parent\" || exit 13; elif test -e \"$parent\" || test -L \"$parent\"; then exit 14; fi; done; printf 'missing\t0'; fi",
            path = shell_quote_word(&path)
        );
        let output = self
            .backend
            .exec(ContainerExecRequest {
                call_id: None,
                command,
                cwd: request.cwd,
                timeout_secs: Some(10),
                output_bytes_cap: None,
                cancellation_token: None,
            })
            .await
            .map_err(|error| tool_error("file", error))?;
        if output.status != 0 {
            return Err(tool_error(
                "file",
                format!(
                    "stat failed: {}",
                    preview_error(&output.stderr, &output.stdout)
                ),
            ));
        }
        let raw = output.stdout.trim();
        let mut parts = raw.split_whitespace();
        let kind = parts
            .next()
            .ok_or_else(|| tool_error("file", "stat returned no metadata"))?;
        if !matches!(kind, "file" | "dir" | "other" | "missing") {
            return Err(tool_error("file", "stat returned an invalid path kind"));
        }
        if kind == "missing" {
            return Ok(None);
        }
        let size = parts
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| tool_error("file", "stat returned an invalid byte length"))?;
        let len = (kind == "file").then_some(size);
        Ok(Some(WorkspaceFileStat {
            path: request.path,
            is_file: kind == "file",
            is_dir: kind == "dir",
            len,
            readonly: None,
            modified_at: None,
        }))
    }

    async fn read_text(&self, request: WorkspaceFileReadRequest) -> Result<String> {
        let bytes = self
            .backend
            .copy_from(ContainerCopyFromRequest {
                path: resolve_container_workspace_path(&request.path, request.cwd.as_deref())?,
                archive: false,
            })
            .await
            .map_err(|error| {
                tool_error(
                    "read_file",
                    format!("failed to read `{}`: {error}", request.path),
                )
            })?;
        String::from_utf8(bytes).map_err(|error| {
            tool_error(
                "read_file",
                format!("failed to decode `{}` as UTF-8: {error}", request.path),
            )
        })
    }

    async fn read_bytes(&self, request: WorkspaceFileReadBytesRequest) -> Result<Vec<u8>> {
        let stat = self
            .stat(WorkspaceFileStatRequest {
                path: request.path.clone(),
                cwd: request.cwd.clone(),
            })
            .await?;
        if !stat.is_file {
            return Err(tool_error(
                "view_image",
                format!("'{}' is not a regular file", request.path),
            ));
        }
        if stat.len.is_some_and(|len| len > request.max_bytes as u64) {
            return Err(tool_error(
                "view_image",
                format!(
                    "'{}' exceeds the {} byte source limit",
                    request.path, request.max_bytes
                ),
            ));
        }
        let bytes = self
            .backend
            .copy_from(ContainerCopyFromRequest {
                path: resolve_container_workspace_path(&request.path, request.cwd.as_deref())?,
                archive: false,
            })
            .await
            .map_err(|error| {
                tool_error(
                    "view_image",
                    format!("failed to read `{}`: {error}", request.path),
                )
            })?;
        if bytes.len() > request.max_bytes {
            return Err(tool_error(
                "view_image",
                format!(
                    "'{}' changed while reading and exceeds the {} byte source limit",
                    request.path, request.max_bytes
                ),
            ));
        }
        Ok(bytes)
    }

    async fn write_text(&self, request: WorkspaceFileWriteRequest) -> Result<()> {
        self.backend
            .copy_to(ContainerCopyToRequest {
                path: resolve_container_workspace_path(&request.path, request.cwd.as_deref())?,
                content: request.content.into_bytes(),
            })
            .await
            .map_err(|error| {
                tool_error(
                    "apply_patch",
                    format!("failed to write `{}`: {error}", request.path),
                )
            })
    }

    async fn remove_file(&self, request: WorkspaceFileRemoveRequest) -> Result<()> {
        let path = resolve_container_workspace_path(&request.path, request.cwd.as_deref())?;
        let command = format!("rm -f -- {}", shell_quote_word(&path));
        let output = self
            .backend
            .exec(ContainerExecRequest {
                call_id: None,
                command,
                cwd: Some("/".to_string()),
                timeout_secs: Some(20),
                output_bytes_cap: None,
                cancellation_token: None,
            })
            .await
            .map_err(|error| tool_error("apply_patch", error))?;
        if output.status != 0 {
            return Err(tool_error(
                "apply_patch",
                format!(
                    "failed to remove `{}`: {}",
                    request.path,
                    preview_error(&output.stderr, &output.stdout)
                ),
            ));
        }
        Ok(())
    }

    async fn list(&self, request: WorkspaceFileListRequest) -> Result<WorkspaceFileListResult> {
        let limit = request.max_files.saturating_add(1);
        let rg_command = format!(
            "if ! test -e {path}; then exit 0; elif command -v rg >/dev/null 2>&1; then rg --files -g {glob} {path} | sort | head -n {limit}; else exit 127; fi",
            path = shell_quote_word(&request.path),
            glob = shell_quote_word(&request.glob),
            limit = limit
        );
        let mut output = self
            .backend
            .exec(ContainerExecRequest {
                call_id: None,
                command: rg_command,
                cwd: request.cwd.clone(),
                timeout_secs: Some(20),
                output_bytes_cap: None,
                cancellation_token: None,
            })
            .await
            .map_err(|error| tool_error("list_files", error))?;
        if output.status == 127 {
            let type_filter = if request.include_dirs { "" } else { "-type f " };
            let command = format!(
                "if test -e {path}; then find {path} {type_filter}-name {glob} | sort | head -n {limit}; fi",
                path = shell_quote_word(&request.path),
                type_filter = type_filter,
                glob = shell_quote_word(&request.glob),
                limit = limit
            );
            output = self
                .backend
                .exec(ContainerExecRequest {
                    call_id: None,
                    command,
                    cwd: request.cwd.clone(),
                    timeout_secs: Some(20),
                    output_bytes_cap: None,
                    cancellation_token: None,
                })
                .await
                .map_err(|error| tool_error("list_files", error))?;
        } else if request.include_dirs {
            let dir_command = format!(
                "if test -e {path}; then find {path} -type d -name {glob} | sort | head -n {limit}; fi",
                path = shell_quote_word(&request.path),
                glob = shell_quote_word(&request.glob),
                limit = limit
            );
            let dirs = self
                .backend
                .exec(ContainerExecRequest {
                    call_id: None,
                    command: dir_command,
                    cwd: request.cwd.clone(),
                    timeout_secs: Some(20),
                    output_bytes_cap: None,
                    cancellation_token: None,
                })
                .await
                .map_err(|error| tool_error("list_files", error))?;
            if dirs.status == 0 {
                output.stdout.push_str(&dirs.stdout);
            }
        }
        if output.status != 0 && output.status != 1 {
            return Err(tool_error(
                "list_files",
                format!(
                    "list_files failed: {}",
                    preview_error(&output.stderr, &output.stdout)
                ),
            ));
        }
        let listed_root = request.path.trim_end_matches('/');
        let mut files = output
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter(|line| line.trim_end_matches('/') != listed_root)
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .take(request.max_files.saturating_add(1))
            .collect::<Vec<_>>();
        let truncated = files.len() > request.max_files;
        files.truncate(request.max_files);
        Ok(WorkspaceFileListResult { files, truncated })
    }
}

use std::sync::Arc;

use crate::tool::container::helpers::preview_error;
use crate::tool::shell::shell_quote_word;
use crate::tool::{
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecRequest,
    tool_error,
};
use pl_protocol::Result;

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadRequest, WorkspaceFileRemoveRequest, WorkspaceFileStat,
    WorkspaceFileStatRequest, WorkspaceFileWriteRequest,
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

    async fn stat(&self, request: WorkspaceFileStatRequest) -> Result<WorkspaceFileStat> {
        let command = format!(
            "if test -f {path}; then printf 'file\t'; wc -c < {path}; elif test -d {path}; then printf 'dir\t0'; else printf 'missing\t0'; fi",
            path = shell_quote_word(&request.path)
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
        let kind = parts.next().unwrap_or("missing");
        if kind == "missing" {
            return Err(tool_error(
                "file",
                format!("file not found: {}", request.path),
            ));
        }
        let len = parts.next().and_then(|value| value.parse::<u64>().ok());
        Ok(WorkspaceFileStat {
            path: request.path,
            is_file: kind == "file",
            is_dir: kind == "dir",
            len,
        })
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

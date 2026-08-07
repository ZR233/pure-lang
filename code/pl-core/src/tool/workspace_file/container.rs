use std::sync::Arc;

use pl_protocol::Result;
use serde::Deserialize;

use crate::tool::container::helpers::{
    bounded_text, preview_error, shell_command, shell_quote, tool_error,
};
use crate::tool::{
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecRequest,
};

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadRequest, WorkspaceFileRemoveRequest, WorkspaceFileSearchMatch,
    WorkspaceFileSearchRequest, WorkspaceFileSearchResult, WorkspaceFileStat,
    WorkspaceFileStatRequest, WorkspaceFileWriteRequest,
};
use super::container_path::resolve_container_workspace_path;

const MAX_SEARCH_OUTPUT_TEXT_BYTES: usize = 4 * 1024;

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
            path = shell_quote(&request.path)
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
        let command = format!("rm -f -- {}", shell_quote(&path));
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
            path = shell_quote(&request.path),
            glob = shell_quote(&request.glob),
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
                path = shell_quote(&request.path),
                type_filter = type_filter,
                glob = shell_quote(&request.glob),
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
                path = shell_quote(&request.path),
                glob = shell_quote(&request.glob),
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

    async fn search(
        &self,
        request: WorkspaceFileSearchRequest,
    ) -> Result<WorkspaceFileSearchResult> {
        let mut args = vec![
            "rg".to_string(),
            "--json".to_string(),
            "--line-number".to_string(),
            "--column".to_string(),
            "--max-count".to_string(),
            request.max_matches.to_string(),
        ];
        if !request.case_sensitive {
            args.push("--ignore-case".to_string());
        }
        if request.literal {
            args.push("--fixed-strings".to_string());
        }
        if request.context_lines > 0 {
            args.push("--context".to_string());
            args.push(request.context_lines.to_string());
        }
        if let Some(glob) = &request.glob {
            args.push("--glob".to_string());
            args.push(glob.clone());
        }
        args.push("--".to_string());
        args.push(request.query.clone());
        args.push(request.path.clone());
        let command = format!(
            "if command -v rg >/dev/null 2>&1; then {rg}; else exit 127; fi",
            rg = shell_command(&args)
        );
        let output = self
            .backend
            .exec(ContainerExecRequest {
                call_id: None,
                command,
                cwd: request.cwd.clone(),
                timeout_secs: Some(30),
                output_bytes_cap: None,
                cancellation_token: None,
            })
            .await
            .map_err(|error| tool_error("search_files", error))?;
        if output.status == 1 {
            return Ok(WorkspaceFileSearchResult {
                matches: Vec::new(),
                truncated: false,
            });
        }
        if output.status == 127 {
            return self.search_with_grep(request).await;
        }
        if output.status != 0 {
            return Err(tool_error(
                "search_files",
                format!(
                    "search_files failed: {}",
                    preview_error(&output.stderr, &output.stdout)
                ),
            ));
        }
        let mut matches = Vec::new();
        for line in output.stdout.lines() {
            if matches.len() >= request.max_matches {
                break;
            }
            let Ok(event) = serde_json::from_str::<RgJsonEvent>(line) else {
                continue;
            };
            if event.kind != "match" {
                continue;
            }
            let Some(data) = event.data else {
                continue;
            };
            let (text, _, _, _) = bounded_text(&data.lines.text, MAX_SEARCH_OUTPUT_TEXT_BYTES, 0);
            let column = data
                .submatches
                .first()
                .map(|item| item.start.saturating_add(1))
                .unwrap_or(1);
            matches.push(WorkspaceFileSearchMatch {
                path: data.path.text,
                line: data.line_number.unwrap_or(0),
                column,
                text,
            });
        }
        let truncated = matches.len() >= request.max_matches;
        Ok(WorkspaceFileSearchResult { matches, truncated })
    }
}

impl<B> ContainerWorkspaceFileBackend<B>
where
    B: ContainerBackend,
{
    async fn search_with_grep(
        &self,
        request: WorkspaceFileSearchRequest,
    ) -> Result<WorkspaceFileSearchResult> {
        let command = grep_search_command(&request);
        let output = self
            .backend
            .exec(ContainerExecRequest {
                call_id: None,
                command,
                cwd: request.cwd,
                timeout_secs: Some(30),
                output_bytes_cap: None,
                cancellation_token: None,
            })
            .await
            .map_err(|error| tool_error("search_files", error))?;
        if output.status != 0 && output.stdout.trim().is_empty() {
            return Ok(WorkspaceFileSearchResult {
                matches: Vec::new(),
                truncated: false,
            });
        }
        let mut matches = Vec::new();
        for raw in output
            .stdout
            .lines()
            .take(request.max_matches.saturating_add(1))
        {
            if matches.len() >= request.max_matches {
                break;
            }
            if let Some((file, rest)) = raw.split_once(':')
                && let Some((line, text)) = rest.split_once(':')
            {
                matches.push(WorkspaceFileSearchMatch {
                    path: file.to_string(),
                    line: line.parse::<usize>().unwrap_or(0),
                    column: 1,
                    text: text.to_string(),
                });
            }
        }
        let truncated = output.stdout.lines().count() > request.max_matches;
        Ok(WorkspaceFileSearchResult { matches, truncated })
    }
}

fn grep_search_command(request: &WorkspaceFileSearchRequest) -> String {
    let mut grep_args = vec![
        "grep".to_string(),
        "-R".to_string(),
        "-n".to_string(),
        "-H".to_string(),
    ];
    if !request.case_sensitive {
        grep_args.push("-i".to_string());
    }
    if request.literal {
        grep_args.push("-F".to_string());
    }
    if let Some(glob) = &request.glob {
        grep_args.push(format!("--include={glob}"));
    }
    grep_args.push("--".to_string());
    grep_args.push(request.query.clone());
    grep_args.push(request.path.clone());
    let grep = shell_command(&grep_args);
    format!("{grep} | head -n {}", request.max_matches.saturating_add(1))
}

#[derive(Debug, Deserialize)]
struct RgJsonEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Option<RgJsonData>,
}

#[derive(Debug, Deserialize)]
struct RgJsonData {
    path: RgJsonText,
    lines: RgJsonText,
    line_number: Option<usize>,
    #[serde(default)]
    submatches: Vec<RgJsonSubmatch>,
}

#[derive(Debug, Deserialize)]
struct RgJsonText {
    text: String,
}

#[derive(Debug, Deserialize)]
struct RgJsonSubmatch {
    start: usize,
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::process::Command;

    use super::*;

    #[test]
    fn grep_fallback_command_uses_native_include_glob() {
        let request = search_request("workspace".to_string());
        let command = grep_search_command(&request);

        assert!(command.contains("--include="));
        assert!(command.contains("*.rs"));
        assert!(!command.contains("--glob"));
    }

    #[cfg(unix)]
    #[test]
    fn grep_fallback_filters_native_glob_in_posix_shell() {
        let root = std::env::temp_dir().join(format!(
            "pl-container-search-fallback-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("match.rs"), "pub static FD_TABLE: usize = 0;\n").unwrap();
        std::fs::write(root.join("match.txt"), "pub static FD_TABLE: usize = 0;\n").unwrap();
        let request = search_request(root.to_string_lossy().into_owned());

        let output = Command::new("sh")
            .arg("-c")
            .arg(grep_search_command(&request))
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();

        assert!(output.status.success());
        assert!(stdout.contains("match.rs"));
        assert!(!stdout.contains("match.txt"));
        let _ = std::fs::remove_dir_all(root);
    }

    fn search_request(path: String) -> WorkspaceFileSearchRequest {
        WorkspaceFileSearchRequest {
            query: "static FD_TABLE".to_string(),
            path,
            cwd: None,
            glob: Some("*.rs".to_string()),
            case_sensitive: true,
            literal: true,
            max_matches: 10,
            context_lines: 0,
        }
    }
}

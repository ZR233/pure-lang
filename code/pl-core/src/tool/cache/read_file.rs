use std::path::Path;

use serde_json::Value;

use super::ToolCachePolicy;
use super::key::{effective_epoch, repository_view};
use crate::tool::ToolOutput;

#[derive(Debug, Clone)]
pub(super) struct ReadFileRequest {
    pub(super) identity: String,
    pub(super) start_line: u64,
    pub(super) requested_end_line: u64,
}

#[derive(Debug, Clone)]
pub(super) struct ReadFileRange {
    identity: String,
    start_line: u64,
    end_line: u64,
    reaches_eof: bool,
}

impl ReadFileRange {
    pub(super) fn from_output(request: &ReadFileRequest, output: &ToolOutput) -> Option<Self> {
        let value = serde_json::from_str::<Value>(&output.description).ok()?;
        let start_line = value.get("startLine")?.as_u64()?;
        let end_line = value.get("endLine")?.as_u64()?;
        if start_line != request.start_line || end_line < start_line {
            return None;
        }
        Some(Self {
            identity: request.identity.clone(),
            start_line,
            end_line,
            reaches_eof: value.get("nextStartLine").is_some_and(Value::is_null),
        })
    }

    pub(super) fn covers(&self, request: &ReadFileRequest) -> bool {
        self.identity == request.identity
            && self.start_line <= request.start_line
            && request.start_line <= self.end_line
            && (self.end_line >= request.requested_end_line || self.reaches_eof)
    }
}

pub(super) fn read_file_request(
    tool_name: &str,
    arguments: &Value,
    workspace_root: &Path,
    policy: ToolCachePolicy,
    workspace_epoch: u64,
) -> Option<ReadFileRequest> {
    if tool_name != "read_file" {
        return None;
    }
    let path = arguments.get("path")?.as_str()?;
    let cwd = arguments.get("cwd").and_then(Value::as_str);
    let start_line = arguments
        .get("startLine")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let max_lines = arguments
        .get("maxLines")
        .and_then(Value::as_u64)
        .unwrap_or(200);
    if start_line == 0 || max_lines == 0 {
        return None;
    }
    let repository_view = repository_view(arguments);
    let epoch = effective_epoch(policy, repository_view, workspace_epoch);
    let identity_arguments = serde_json::json!({
        "path": path,
        "cwd": cwd,
    });
    let identity = crate::working_set::canonical_content_hash(
        format!(
            "read_file\0{}\0{}\0{repository_view:?}\0{epoch}",
            workspace_root.display(),
            crate::working_set::canonical_json_string(&identity_arguments),
        )
        .as_bytes(),
    );
    Some(ReadFileRequest {
        identity,
        start_line,
        requested_end_line: start_line.saturating_add(max_lines.saturating_sub(1)),
    })
}

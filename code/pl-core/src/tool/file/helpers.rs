use std::path::{Path, PathBuf};

use super::path::WorkspacePaths;
use crate::tool::truncation::{OutputTruncation, TruncatedOutput};
use crate::tool::{ToolCallContext, ToolResult, ToolWorkspace, tool_error};
use pl_protocol::PureError;

pub(super) fn text_output(description: String) -> ToolResult {
    let stdout = TruncatedOutput {
        original_length: description.len(),
        content: description,
        was_truncated: false,
    };
    ToolResult::from_runtime_text(
        stdout.content.clone(),
        OutputTruncation {
            stdout,
            stderr: TruncatedOutput::empty(),
        },
        PathBuf::new(),
        Some(0),
        false,
        Vec::new(),
    )
}

pub(super) async fn workspace(
    runtime: &ToolWorkspace,
    context: &ToolCallContext,
) -> Result<WorkspacePaths, PureError> {
    WorkspacePaths::new(
        runtime.root().to_path_buf(),
        runtime.allows_workspace_escape(context),
    )
    .await
}

pub(super) fn path_type(metadata: &std::fs::Metadata) -> &'static str {
    if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else {
        "other"
    }
}

pub(super) async fn ensure_overwrite(
    path: &Path,
    overwrite: bool,
    tool: &str,
) -> Result<(), PureError> {
    if !overwrite && tokio::fs::try_exists(path).await? {
        return Err(tool_error(
            tool,
            format!(
                "target '{}' already exists; use overwrite mode or apply_patch for an intentional replacement",
                path.display()
            ),
        ));
    }
    Ok(())
}

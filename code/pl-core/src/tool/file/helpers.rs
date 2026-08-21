use std::path::{Path, PathBuf};

use super::path::WorkspacePaths;
use crate::tool::truncation::{OutputTruncation, TruncatedOutput};
use crate::tool::{ToolContext, ToolOutput, tool_error};
use pl_protocol::PureError;

pub(super) fn text_output(description: String) -> ToolOutput {
    let stdout = TruncatedOutput {
        original_length: description.len(),
        content: description,
        was_truncated: false,
    };
    ToolOutput {
        description: stdout.content.clone(),
        truncated: OutputTruncation {
            stdout,
            stderr: TruncatedOutput::empty(),
        },
        output_file: PathBuf::new(),
        exit_code: Some(0),
        timed_out: false,
        runtime_events: Vec::new(),
    }
}

pub(super) async fn workspace(context: &ToolContext) -> Result<WorkspacePaths, PureError> {
    WorkspacePaths::new(
        context.workspace.root().to_path_buf(),
        context.allows_workspace_escape(),
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

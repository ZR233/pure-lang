use std::path::{Path, PathBuf};

use pl_protocol::PureError;

use super::path::WorkspacePaths;
use crate::tool::truncation::{OutputTruncation, TruncatedOutput};
use crate::tool::{ToolContext, ToolOutput};

pub(super) fn parse_input<T: serde::de::DeserializeOwned>(
    arguments: serde_json::Value,
    tool: &str,
) -> Result<T, PureError> {
    serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: format!("invalid input: {error}. Check the tool schema and remove unknown fields"),
    })
}

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

pub(super) fn tool_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
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

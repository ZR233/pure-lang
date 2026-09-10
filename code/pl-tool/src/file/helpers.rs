use crate::tool_error;
use std::path::Path;

use super::path::WorkspacePaths;
use crate::workspace::ToolWorkspace;
use pl_core::tool::opaque::CallContext;

use pl_protocol::PureError;

pub(super) fn text_output(description: String) -> pl_core::tool::ToolOutput {
    pl_core::tool::ToolOutput::new(
        pl_core::context::OpaquePayload::text(description.clone()),
        vec![pl_core::context::ContextContent::Text {
            text: description.into(),
        }],
    )
}

pub(super) async fn workspace(
    runtime: &ToolWorkspace,
    context: &CallContext,
) -> Result<WorkspacePaths, PureError> {
    WorkspacePaths::new(
        runtime.root().to_path_buf(),
        runtime.allows_workspace_escape(context),
    )
    .await
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

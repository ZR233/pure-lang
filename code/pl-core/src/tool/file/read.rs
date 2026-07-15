use std::time::UNIX_EPOCH;

use pl_protocol::PureError;

use super::helpers::{parse_input, path_type, text_output, tool_error, workspace};
use super::input::{PathInput, path_schema};
use crate::tool::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

#[derive(Debug)]
pub struct StatPathTool;

impl Tool for StatPathTool {
    fn name(&self) -> &str {
        "stat_path"
    }

    fn description(&self) -> &str {
        "Return metadata for a workspace path, or `exists: false` when the path is absent."
    }

    fn input_schema(&self) -> serde_json::Value {
        path_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let input: PathInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_existing_or_parent(&input.path).await?;
            let metadata = match tokio::fs::metadata(&path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(text_output(
                        serde_json::json!({
                            "path": paths.display_relative(&path),
                            "exists": false,
                        })
                        .to_string(),
                    ));
                }
                Err(error) => {
                    return Err(tool_error(
                        self.name(),
                        format!("failed to inspect path '{}': {error}", input.path),
                    ));
                }
            };
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64);
            Ok(text_output(
                serde_json::json!({
                    "path": paths.display_relative(&path),
                    "exists": true,
                    "type": path_type(&metadata),
                    "len": metadata.len(),
                    "readonly": metadata.permissions().readonly(),
                    "modifiedAt": modified_at,
                })
                .to_string(),
            ))
        })
    }
}

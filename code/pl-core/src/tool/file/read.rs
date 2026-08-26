use std::time::UNIX_EPOCH;

use futures::FutureExt;
use pl_protocol::PureError;

use super::helpers::*;
use super::input::PathInput;
use crate::tool::{
    BoxFuture, Tool, ToolCallContext, ToolInput, ToolResult, ToolWorkspace, TypedTool,
    deserialize_tool_input, tool_error,
};
use crate::turn::ToolEffect;

#[derive(Debug, Clone)]
pub struct StatPathTool {
    workspace: ToolWorkspace,
}

impl StatPathTool {
    pub fn new(workspace: ToolWorkspace) -> Self {
        Self { workspace }
    }
}

impl Tool for StatPathTool {
    fn name(&self) -> &str {
        "stat_path"
    }

    fn description(&self) -> &str {
        "Return metadata for a workspace path, or `exists: false` when the path is absent."
    }

    fn input_schema(&self) -> serde_json::Value {
        TypedTool::<PathInput>::new(self.name(), self.description()).input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn cache_policy(&self, _arguments: &serde_json::Value) -> crate::tool::cache::ToolCachePolicy {
        crate::tool::cache::ToolCachePolicy::UntilWorkspaceMutation
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            let input: PathInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&self.workspace, &context).await?;
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
        }
        .boxed()
    }
}

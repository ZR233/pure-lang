use std::future::Future;
use std::time::UNIX_EPOCH;

use pl_protocol::PureError;

use super::helpers::*;
use super::input::PathInput;
use crate::tool::{StaticTool, ToolCallContext, ToolPolicy, ToolResult, ToolWorkspace, tool_error};

#[derive(Debug, Clone)]
pub struct StatPathTool {
    workspace: ToolWorkspace,
}

impl StatPathTool {
    pub fn new(workspace: ToolWorkspace) -> Self {
        Self { workspace }
    }
}

impl StaticTool for StatPathTool {
    type Input = PathInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("stat_path"),
            "Return metadata for a workspace path, or `exists: false` when the path is absent.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
            .with_parallel_tool_calls()
            .with_cache_policy(crate::tool::cache::ToolCachePolicy::UntilWorkspaceMutation)
    }

    fn execute(
        &self,
        input: PathInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
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
                        "stat_path",
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
    }
}

mod apply_patch;
mod helpers;
mod input;
mod path;
mod read;
mod write;

#[cfg(test)]
mod tests;

use pl_model::ToolSchema;

use apply_patch::{APPLY_PATCH_LARK_GRAMMAR, apply_patch};

use crate::tool::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

pub use read::{ListFilesTool, ReadFileTool, SearchFilesTool, StatPathTool};
pub use write::{CopyPathTool, CreateDirectoryTool, DeletePathTool, MovePathTool, WriteFileTool};

#[derive(Debug)]
pub struct ApplyPatchTool;

const APPLY_PATCH_TOOL_DESCRIPTION: &str = "Apply a Codex-style patch to workspace files. The patch must begin with *** Begin Patch and end with *** End Patch. File operations must use *** Add File:, *** Delete File:, or *** Update File: hunk headers. Do not use ---/+++ unified diff, *** File:, or natural-language edit instructions such as Insert after. If a patch fails, read the target file again and retry with a smaller patch based on current content; do not repeat the same failed patch. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

const APPLY_PATCH_INPUT_DESCRIPTION: &str = "Complete Codex-style patch text beginning with *** Begin Patch and ending with *** End Patch. File operations must use *** Add File:, *** Delete File:, or *** Update File:. Do not use ---/+++ unified diff, *** File:, or natural-language edit instructions such as Insert after. If the previous apply_patch failed, first read the target file again and then send a smaller patch using current file content. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

const APPLY_PATCH_CUSTOM_TOOL_DESCRIPTION: &str = "Use the `apply_patch` tool to edit workspace files. This is a FREEFORM tool, so do not wrap the patch in JSON. The patch must begin with *** Begin Patch, end with *** End Patch, and each file operation must use *** Add File:, *** Delete File:, or *** Update File:. Do not use ---/+++ unified diff, *** File:, or natural-language edit instructions such as Insert after. If a patch fails, read the file again and retry with a smaller patch based on current content. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        APPLY_PATCH_TOOL_DESCRIPTION
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": APPLY_PATCH_INPUT_DESCRIPTION
                }
            },
            "required": ["patch"],
            "additionalProperties": false
        })
    }

    fn to_schema(&self) -> ToolSchema {
        ToolSchema::custom_grammar(
            self.name(),
            APPLY_PATCH_CUSTOM_TOOL_DESCRIPTION,
            "lark",
            APPLY_PATCH_LARK_GRAMMAR,
        )
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, pl_protocol::PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let patch = extract_patch(input.arguments)?;
            let paths = helpers::workspace(&context).await?;
            let outcome = apply_patch(&patch, &paths).await?;
            let summary = outcome.summary(&paths);
            Ok(helpers::text_output(summary))
        })
    }
}

fn extract_patch(arguments: serde_json::Value) -> Result<String, pl_protocol::PureError> {
    match arguments {
        serde_json::Value::String(patch) => Ok(patch),
        serde_json::Value::Object(mut object) => object
            .remove("patch")
            .or_else(|| object.remove("input"))
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .ok_or_else(|| helpers::tool_error("apply_patch", "missing patch input")),
        _ => Err(helpers::tool_error(
            "apply_patch",
            "invalid apply_patch input",
        )),
    }
}

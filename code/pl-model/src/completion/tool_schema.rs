//! Provider compatibility projection for canonical tool specifications.

use pl_protocol::ToolSpec;

const APPLY_PATCH_FUNCTION_FALLBACK_DESCRIPTION: &str = "Complete Codex-style apply_patch text beginning with *** Begin Patch and ending with *** End Patch. Each file operation must use one of these hunk headers: *** Add File: <path>, *** Delete File: <path>, or *** Update File: <path>. Do not use ---/+++ unified diff, *** File: metadata, or natural-language edit instructions such as Insert after. If a previous patch failed, read the target file again and retry with a smaller patch based on current content; do not repeat the same failed patch. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

pub(crate) fn provider_compatible_tool(tool: ToolSpec, supports_custom_tools: bool) -> ToolSpec {
    if supports_custom_tools {
        return tool;
    }

    match tool {
        ToolSpec::Custom {
            name,
            description,
            allowed_callers,
            output_schema,
            ..
        } if name == "apply_patch" => ToolSpec::Function {
            name,
            description,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": APPLY_PATCH_FUNCTION_FALLBACK_DESCRIPTION
                    }
                },
                "required": ["input"],
                "additionalProperties": false
            }),
            allowed_callers,
            output_schema,
        },
        ToolSpec::Custom {
            name,
            description,
            allowed_callers,
            output_schema,
            ..
        } => ToolSpec::Function {
            name,
            description,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                },
                "required": ["input"],
                "additionalProperties": false
            }),
            allowed_callers,
            output_schema,
        },
        tool => tool,
    }
}

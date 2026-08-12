use pl_model::ToolSchema;
use serde_json::{Value, json};

pub const TOOL_READ_FILE: &str = "read_file";
pub const TOOL_LIST_FILES: &str = "list_files";
pub const TOOL_APPLY_PATCH: &str = "apply_patch";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceFileToolKind {
    ReadFile,
    ListFiles,
    ApplyPatch,
}

impl WorkspaceFileToolKind {
    pub fn all() -> &'static [Self] {
        &[Self::ReadFile, Self::ListFiles, Self::ApplyPatch]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            TOOL_READ_FILE => Some(Self::ReadFile),
            TOOL_LIST_FILES => Some(Self::ListFiles),
            TOOL_APPLY_PATCH => Some(Self::ApplyPatch),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::ReadFile => TOOL_READ_FILE,
            Self::ListFiles => TOOL_LIST_FILES,
            Self::ApplyPatch => TOOL_APPLY_PATCH,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ReadFile => {
                "Read a UTF-8 text file by 1-based source lines. Use startLine and nextStartLine for deterministic paging; each call returns at most 500 lines."
            }
            Self::ListFiles => {
                "List descendants from the agent workspace with an optional glob and bounded result count. Omit path or use `.` for the workspace root. For pagination, reuse the exact nextCursor and keep path, cwd, glob, and includeDirs unchanged; limit may change. Any intervening workspace write, exec, or Git mutation invalidates existing cursors. A missing or empty workspace directory returns an empty list."
            }
            Self::ApplyPatch => {
                "Apply a Codex-style patch to workspace files. The input field must contain a complete patch beginning with *** Begin Patch and ending with *** End Patch. Every Update hunk line starts with a control prefix: space for context, `-` for deletion, or `+` for addition. Preserve a leading `-` or `+` in the file content after that prefix; for example, replace Markdown `- old` with `-- old` and `+- new`."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::ReadFile => object_schema(vec![
                ("path", json!({ "type": "string" }), true),
                ("cwd", json!({ "type": "string" }), false),
                (
                    "startLine",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                (
                    "maxLines",
                    json!({
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "description": "Maximum source lines to return. Defaults to 200."
                    }),
                    false,
                ),
            ]),
            Self::ListFiles => object_schema(vec![
                ("path", json!({ "type": "string" }), false),
                ("cwd", json!({ "type": "string" }), false),
                (
                    "glob",
                    json!({
                        "type": "string",
                        "description": "Optional file glob filter, such as `*.rs`. Omitted or blank uses `*`."
                    }),
                    false,
                ),
                (
                    "limit",
                    json!({ "type": "integer", "minimum": 1, "maximum": 200 }),
                    false,
                ),
                (
                    "cursor",
                    json!({
                        "type": "string",
                        "description": "Exact nextCursor from the corresponding previous page. Omit it on the first page. When set, keep path, cwd, glob, and includeDirs identical to the call that produced it; limit may change. Never mix cursors between calls. Any intervening workspace write, exec, or Git mutation invalidates it."
                    }),
                    false,
                ),
                (
                    "includeDirs",
                    json!({
                        "type": "boolean",
                        "description": "Whether directory entries should be included in addition to files."
                    }),
                    false,
                ),
            ]),
            Self::ApplyPatch => object_schema(vec![
                (
                    "input",
                    json!({
                        "type": "string",
                        "description": "The entire contents of the apply_patch command. In an Update hunk, prefix each line with space (context), `-` (deletion), or `+` (addition). Keep any leading `-` or `+` from the file content after that control prefix; replacing Markdown `- old` with `- new` requires `-- old` and `+- new`."
                    }),
                    true,
                ),
                ("cwd", json!({ "type": "string" }), false),
            ]),
        }
    }

    pub fn supports_parallel_tool_calls(self) -> bool {
        !matches!(self, Self::ApplyPatch)
    }

    pub fn to_schema(self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

fn object_schema(fields: Vec<(&str, Value, bool)>) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, schema, is_required) in fields {
        properties.insert(name.to_string(), schema);
        if is_required {
            required.push(Value::String(name.to_string()));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_descriptions_name_cursor_bound_parameters() {
        let list = WorkspaceFileToolKind::ListFiles;
        let list_schema = list.input_schema();
        let list_cursor = list_schema["properties"]["cursor"]["description"]
            .as_str()
            .unwrap();
        assert!(
            list.description()
                .contains("keep path, cwd, glob, and includeDirs unchanged")
        );
        assert!(list_cursor.contains("keep path, cwd, glob, and includeDirs identical"));
        assert!(list_cursor.contains("limit may change"));
    }

    #[test]
    fn apply_patch_description_explains_content_prefixes() {
        let kind = WorkspaceFileToolKind::ApplyPatch;
        let schema = kind.input_schema();
        let input = schema["properties"]["input"]["description"]
            .as_str()
            .unwrap();

        assert!(kind.description().contains("control prefix"));
        assert!(kind.description().contains("`-- old` and `+- new`"));
        assert!(input.contains("`-- old` and `+- new`"));
    }
}

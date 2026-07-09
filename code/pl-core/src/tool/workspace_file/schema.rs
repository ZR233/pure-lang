use pl_model::ToolSchema;
use serde_json::{Value, json};

pub const TOOL_READ_FILE: &str = "read_file";
pub const TOOL_LIST_FILES: &str = "list_files";
pub const TOOL_SEARCH_FILES: &str = "search_files";
pub const TOOL_APPLY_PATCH: &str = "apply_patch";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceFileToolKind {
    ReadFile,
    ListFiles,
    SearchFiles,
    ApplyPatch,
}

impl WorkspaceFileToolKind {
    pub fn all() -> &'static [Self] {
        &[
            Self::ReadFile,
            Self::ListFiles,
            Self::SearchFiles,
            Self::ApplyPatch,
        ]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            TOOL_READ_FILE => Some(Self::ReadFile),
            TOOL_LIST_FILES => Some(Self::ListFiles),
            TOOL_SEARCH_FILES => Some(Self::SearchFiles),
            TOOL_APPLY_PATCH => Some(Self::ApplyPatch),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::ReadFile => TOOL_READ_FILE,
            Self::ListFiles => TOOL_LIST_FILES,
            Self::SearchFiles => TOOL_SEARCH_FILES,
            Self::ApplyPatch => TOOL_APPLY_PATCH,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ReadFile => {
                "Read a UTF-8 text file from the agent workspace with bounded output. Use lineStart/lineCount for source files or offset/maxBytes for byte paging."
            }
            Self::ListFiles => {
                "List files from the agent workspace with an optional glob and bounded result count."
            }
            Self::SearchFiles => {
                "Search workspace file contents and return structured matches with path, line, column, and text."
            }
            Self::ApplyPatch => {
                "Apply a Codex-style patch to workspace files. The input field must contain a complete patch beginning with *** Begin Patch and ending with *** End Patch."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::ReadFile => object_schema(vec![
                ("path", json!({ "type": "string" }), true),
                ("cwd", json!({ "type": "string" }), false),
                (
                    "lineStart",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                (
                    "lineCount",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                ("offset", json!({ "type": "integer", "minimum": 0 }), false),
                (
                    "maxBytes",
                    json!({ "type": "integer", "minimum": 1 }),
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
                        "description": "Optional file glob filter, such as `*.rs`."
                    }),
                    false,
                ),
                (
                    "maxFiles",
                    json!({ "type": "integer", "minimum": 1 }),
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
            Self::SearchFiles => object_schema(vec![
                (
                    "query",
                    json!({
                        "type": "string",
                        "description": "Text or pattern to search for."
                    }),
                    true,
                ),
                (
                    "path",
                    json!({
                        "type": "string",
                        "description": "Directory or file path to search. Defaults to the current working directory."
                    }),
                    false,
                ),
                ("cwd", json!({ "type": "string" }), false),
                (
                    "glob",
                    json!({
                        "type": "string",
                        "description": "Optional file glob filter, such as `*.rs`."
                    }),
                    false,
                ),
                (
                    "caseSensitive",
                    json!({
                        "type": "boolean",
                        "description": "Whether matching should be case-sensitive."
                    }),
                    false,
                ),
                (
                    "literal",
                    json!({
                        "type": "boolean",
                        "description": "Treat query as literal text instead of a regular expression."
                    }),
                    false,
                ),
                (
                    "maxMatches",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                (
                    "contextLines",
                    json!({
                        "type": "integer",
                        "minimum": 0,
                        "description": "Number of context lines to include before and after each match."
                    }),
                    false,
                ),
            ]),
            Self::ApplyPatch => object_schema(vec![
                (
                    "input",
                    json!({
                        "type": "string",
                        "description": "The entire contents of the apply_patch command."
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

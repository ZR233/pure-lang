use pl_model::ToolSchema;
use serde_json::{Value, json};

use super::helpers::object_schema;
use super::patch::APPLY_PATCH_DESCRIPTION;

pub const TOOL_CONTAINER_EXEC: &str = "container_exec";
pub const TOOL_CONTAINER_CP_UPLOAD: &str = "container_cp_upload";
pub const TOOL_CONTAINER_CP_DOWNLOAD: &str = "container_cp_download";

pub(super) const TOOL_READ_FILE: &str = "read_file";
pub(super) const TOOL_LIST_FILES: &str = "list_files";
pub(super) const TOOL_SEARCH_FILES: &str = "search_files";
pub(super) const TOOL_APPLY_PATCH: &str = "apply_patch";

/// pl-core 共享的容器/file 工具类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerToolKind {
    Exec,
    ReadFile,
    ListFiles,
    SearchFiles,
    ApplyPatch,
    CopyUpload,
    CopyDownload,
}

impl ContainerToolKind {
    pub fn all() -> &'static [Self] {
        &[
            Self::Exec,
            Self::ReadFile,
            Self::ListFiles,
            Self::SearchFiles,
            Self::ApplyPatch,
            Self::CopyUpload,
            Self::CopyDownload,
        ]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let normalized;
        let name = if name.contains('.') {
            normalized = name.replace('.', "_");
            normalized.as_str()
        } else {
            name
        };
        match name {
            TOOL_CONTAINER_EXEC => Some(Self::Exec),
            TOOL_READ_FILE => Some(Self::ReadFile),
            TOOL_LIST_FILES => Some(Self::ListFiles),
            TOOL_SEARCH_FILES => Some(Self::SearchFiles),
            TOOL_APPLY_PATCH => Some(Self::ApplyPatch),
            TOOL_CONTAINER_CP_UPLOAD => Some(Self::CopyUpload),
            TOOL_CONTAINER_CP_DOWNLOAD => Some(Self::CopyDownload),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Exec => TOOL_CONTAINER_EXEC,
            Self::ReadFile => TOOL_READ_FILE,
            Self::ListFiles => TOOL_LIST_FILES,
            Self::SearchFiles => TOOL_SEARCH_FILES,
            Self::ApplyPatch => TOOL_APPLY_PATCH,
            Self::CopyUpload => TOOL_CONTAINER_CP_UPLOAD,
            Self::CopyDownload => TOOL_CONTAINER_CP_DOWNLOAD,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Exec => {
                "Execute a shell command inside this agent's Docker container. timeout_secs is optional; omit it for no command time limit."
            }
            Self::ReadFile => {
                "Read a text file inside this agent's Docker container with bounded output. Use line_start/line_count for source files or offset/max_bytes for byte paging."
            }
            Self::ListFiles => {
                "List files inside this agent's Docker container with bounded output."
            }
            Self::SearchFiles => {
                "Search file contents inside this agent's Docker container. Intended for ripgrep-style content search with optional path and glob filters."
            }
            Self::ApplyPatch => APPLY_PATCH_DESCRIPTION,
            Self::CopyUpload => "Write a base64 encoded file into this agent's Docker container.",
            Self::CopyDownload => {
                "Export a file or directory from this agent's Docker container as a base64 encoded tar stream."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::Exec => object_schema(vec![
                ("command", json!({ "type": "string" }), true),
                ("cwd", json!({ "type": "string" }), false),
                (
                    "timeout_secs",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
            ]),
            Self::ReadFile => object_schema(vec![
                ("path", json!({ "type": "string" }), true),
                ("cwd", json!({ "type": "string" }), false),
                (
                    "line_start",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                (
                    "line_count",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                ("offset", json!({ "type": "integer", "minimum": 0 }), false),
                (
                    "max_bytes",
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
                    "max_files",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                (
                    "include_dirs",
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
                    "case_sensitive",
                    json!({
                        "type": "boolean",
                        "description": "Whether matching should be case-sensitive."
                    }),
                    false,
                ),
                (
                    "max_matches",
                    json!({ "type": "integer", "minimum": 1 }),
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
                    "context_lines",
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
            Self::CopyUpload => object_schema(vec![
                ("path", json!({ "type": "string" }), true),
                ("content_base64", json!({ "type": "string" }), true),
            ]),
            Self::CopyDownload => object_schema(vec![("path", json!({ "type": "string" }), true)]),
        }
    }

    pub fn to_schema(self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

use pl_model::ToolSpec;
use serde_json::Value;

use crate::tool::TypedTool;
use crate::tool::cache::ToolCachePolicy;
use crate::turn::ToolEffect;

use super::ops::{ApplyPatchInput, ListFilesInput, ReadFileInput};

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

    /// 该类别工具的副作用声明。
    pub fn effect(self) -> ToolEffect {
        match self {
            Self::ReadFile | Self::ListFiles => ToolEffect::Read,
            Self::ApplyPatch => ToolEffect::WorkspaceWrite,
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
            Self::ReadFile => {
                TypedTool::<ReadFileInput>::new(self.name(), self.description()).input_schema()
            }
            Self::ListFiles => {
                TypedTool::<ListFilesInput>::new(self.name(), self.description()).input_schema()
            }
            Self::ApplyPatch => {
                TypedTool::<ApplyPatchInput>::new(self.name(), self.description()).input_schema()
            }
        }
    }

    pub fn supports_parallel_tool_calls(self) -> bool {
        !matches!(self, Self::ApplyPatch)
    }

    pub fn cache_policy(self) -> ToolCachePolicy {
        match self {
            Self::ReadFile | Self::ListFiles => ToolCachePolicy::UntilWorkspaceMutation,
            Self::ApplyPatch => ToolCachePolicy::Never,
        }
    }

    pub fn to_spec(self) -> ToolSpec {
        ToolSpec::function(self.name(), self.description(), self.input_schema())
    }
}

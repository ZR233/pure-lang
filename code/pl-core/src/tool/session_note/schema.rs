use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::tool::TypedTool;

pub const TOOL_READ_SESSION_NOTE: &str = "read_session_note";
pub const TOOL_SEARCH_SESSION_NOTE: &str = "search_session_note";
pub const TOOL_WRITE_SESSION_NOTE: &str = "write_session_note";
pub const TOOL_APPLY_SESSION_NOTE_PATCH: &str = "apply_session_note_patch";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionNoteToolKind {
    Read,
    Search,
    Write,
    ApplyPatch,
}

impl SessionNoteToolKind {
    pub const fn all() -> &'static [Self] {
        &[Self::Read, Self::Search, Self::Write, Self::ApplyPatch]
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => TOOL_READ_SESSION_NOTE,
            Self::Search => TOOL_SEARCH_SESSION_NOTE,
            Self::Write => TOOL_WRITE_SESSION_NOTE,
            Self::ApplyPatch => TOOL_APPLY_SESSION_NOTE_PATCH,
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Read => {
                "Read a targeted line range from the persistent session note. Reading every page is not required."
            }
            Self::Search => {
                "Search the persistent session note with a line-oriented literal or regular expression query before reading targeted ranges."
            }
            Self::Write => {
                "Create or replace the persistent session note when its revision still matches."
            }
            Self::ApplyPatch => {
                "Atomically edit the persistent session note with a Codex-style patch for session-note.md."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::Read => {
                TypedTool::<ReadInput>::new(self.name(), self.description()).input_schema()
            }
            Self::Search => {
                TypedTool::<SearchInput>::new(self.name(), self.description()).input_schema()
            }
            Self::Write => {
                TypedTool::<WriteInput>::new(self.name(), self.description()).input_schema()
            }
            Self::ApplyPatch => {
                TypedTool::<ApplyPatchInput>::new(self.name(), self.description()).input_schema()
            }
        }
    }

    pub const fn supports_parallel_tool_calls(self) -> bool {
        matches!(self, Self::Read | Self::Search)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReadInput {
    /// 1-based first line to return; defaults to 1.
    #[schemars(range(min = 1))]
    pub(super) start_line: Option<usize>,
    /// Maximum lines to return; defaults to 200.
    #[schemars(range(min = 1, max = 500))]
    pub(super) max_lines: Option<usize>,
    /// Optional revision guard.
    pub(super) expected_revision: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SearchInput {
    /// Literal text or regular expression to find.
    #[schemars(length(max = 4096))]
    pub(super) query: String,
    /// Whether matching is case-sensitive; defaults to true.
    pub(super) case_sensitive: Option<bool>,
    /// Treat query as literal text instead of a regular expression.
    pub(super) literal: Option<bool>,
    /// Context lines around each match.
    #[schemars(range(max = 20))]
    pub(super) context_lines: Option<usize>,
    /// Maximum matches in this page.
    #[schemars(range(min = 1, max = 200))]
    pub(super) limit: Option<usize>,
    /// Omit on the first page; later pass the exact returned nextCursor.
    pub(super) cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ExpectedRevisionInput {
    /// Revision that must still be current when the mutation is applied.
    expected_revision: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct WriteInput {
    /// Complete replacement note content.
    pub(super) content: String,
    #[serde(flatten)]
    revision: ExpectedRevisionInput,
}

impl WriteInput {
    pub(super) fn expected_revision(&self) -> u64 {
        self.revision.expected_revision
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApplyPatchInput {
    /// Codex-style patch targeting session-note.md.
    pub(super) patch: String,
    #[serde(flatten)]
    revision: ExpectedRevisionInput,
}

impl ApplyPatchInput {
    pub(super) fn expected_revision(&self) -> u64 {
        self.revision.expected_revision
    }
}

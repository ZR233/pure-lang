use serde_json::{Value, json};

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
            Self::Read => object_schema(
                [
                    ("startLine", json!({"type": "integer", "minimum": 1})),
                    (
                        "maxLines",
                        json!({"type": "integer", "minimum": 1, "maximum": 500}),
                    ),
                    ("expectedRevision", json!({"type": "integer", "minimum": 0})),
                ],
                &[],
            ),
            Self::Search => object_schema(
                [
                    ("query", json!({"type": "string", "maxLength": 4096})),
                    ("caseSensitive", json!({"type": "boolean"})),
                    ("literal", json!({"type": "boolean"})),
                    (
                        "contextLines",
                        json!({"type": "integer", "minimum": 0, "maximum": 20}),
                    ),
                    (
                        "limit",
                        json!({"type": "integer", "minimum": 1, "maximum": 200}),
                    ),
                    ("cursor", json!({"type": "string"})),
                ],
                &["query"],
            ),
            Self::Write => object_schema(
                [
                    ("content", json!({"type": "string"})),
                    ("expectedRevision", json!({"type": "integer", "minimum": 0})),
                ],
                &["content", "expectedRevision"],
            ),
            Self::ApplyPatch => object_schema(
                [
                    ("patch", json!({"type": "string"})),
                    ("expectedRevision", json!({"type": "integer", "minimum": 0})),
                ],
                &["patch", "expectedRevision"],
            ),
        }
    }

    pub const fn supports_parallel_tool_calls(self) -> bool {
        matches!(self, Self::Read | Self::Search)
    }
}

fn object_schema<const N: usize>(fields: [(&str, Value); N], required: &[&str]) -> Value {
    let properties = fields
        .into_iter()
        .map(|(name, schema)| (name.to_string(), schema))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

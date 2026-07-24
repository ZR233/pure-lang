use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct WriteFileInput {
    pub path: String,
    pub content: String,
    pub mode: WriteMode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum WriteMode {
    Create,
    Overwrite,
    Append,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PathInput {
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeletePathInput {
    pub path: String,
    pub mode: DeleteMode,
}

impl DeletePathInput {
    pub fn delete_mode(&self) -> DeleteMode {
        self.mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum DeleteMode {
    File,
    EmptyDirectory,
    RecursiveDirectory,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CopyMoveInput {
    pub from: String,
    pub to: String,
    pub collision: PathCollision,
}

impl CopyMoveInput {
    pub fn collision(&self) -> PathCollision {
        self.collision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum PathCollision {
    FailIfExists,
    Overwrite,
}

pub(super) fn path_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

pub(super) fn copy_move_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "from": { "type": "string" },
            "to": { "type": "string" },
            "collision": {
                "type": "string",
                "enum": ["failIfExists", "overwrite"]
            }
        },
        "required": ["from", "to", "collision"],
        "additionalProperties": false
    })
}

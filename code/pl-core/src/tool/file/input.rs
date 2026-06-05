use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReadFileInput {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub(super) struct ListFilesInput {
    pub path: Option<String>,
    pub depth: Option<usize>,
    pub pattern: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchFilesInput {
    pub query: String,
    pub path: Option<String>,
    pub pattern: Option<String>,
    pub max_results: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PathInput {
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeletePathInput {
    pub path: String,
    pub recursive: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CopyMoveInput {
    pub from: String,
    pub to: String,
    pub overwrite: Option<bool>,
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
            "overwrite": { "type": "boolean" }
        },
        "required": ["from", "to"],
        "additionalProperties": false
    })
}

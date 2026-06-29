use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReadFileInput {
    pub path: String,
    /// 1-based 起始行号，默认 1（第 1 行）。
    pub line_offset: Option<usize>,
    /// 最多读取的行数；`None` 表示读到文件末尾。
    pub max_lines: Option<usize>,
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
    pub pattern: String,
    pub path: Option<String>,
    pub file_pattern: Option<String>,
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
    pub mode: Option<DeleteMode>,
    /// Legacy compatibility for old tool calls. New schemas expose `mode`.
    pub recursive: Option<bool>,
}

impl DeletePathInput {
    pub fn delete_mode(&self) -> DeleteMode {
        self.mode.unwrap_or_else(|| {
            if self.recursive.unwrap_or(false) {
                DeleteMode::RecursiveDirectory
            } else {
                DeleteMode::File
            }
        })
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
#[serde(rename_all = "camelCase")]
pub(super) struct CopyMoveInput {
    pub from: String,
    pub to: String,
    pub collision: Option<PathCollision>,
    /// Legacy compatibility for old tool calls. New schemas expose `collision`.
    pub overwrite: Option<bool>,
}

impl CopyMoveInput {
    pub fn collision(&self) -> PathCollision {
        self.collision.unwrap_or_else(|| {
            if self.overwrite.unwrap_or(false) {
                PathCollision::Overwrite
            } else {
                PathCollision::FailIfExists
            }
        })
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

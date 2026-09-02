use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteFileInput {
    /// Workspace-relative or permitted absolute destination path.
    pub path: String,
    /// UTF-8 text content.
    pub content: String,
    /// Create, replace, or append behavior.
    pub mode: WriteMode,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WriteMode {
    Create,
    Overwrite,
    Append,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathInput {
    /// Workspace-relative or permitted absolute path.
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeletePathInput {
    /// Workspace-relative or permitted absolute path.
    pub path: String,
    /// Explicit file or directory deletion mode.
    pub mode: DeleteMode,
}

impl DeletePathInput {
    pub fn delete_mode(&self) -> DeleteMode {
        self.mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DeleteMode {
    File,
    EmptyDirectory,
    RecursiveDirectory,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyMoveInput {
    /// Existing source path.
    pub from: String,
    /// Destination path.
    pub to: String,
    /// Behavior when the destination already exists.
    pub collision: PathCollision,
}

impl CopyMoveInput {
    pub fn collision(&self) -> PathCollision {
        self.collision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PathCollision {
    FailIfExists,
    Overwrite,
}

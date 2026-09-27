use schemars::JsonSchema;
use serde::Deserialize;
pub const TOOL_VIEW_IMAGE: &str = "view_image";
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewImageInput {
    /// Image path in the current local or SSH workspace. Use a workspace-relative POSIX path for SSH.
    pub(crate) path: String,
}

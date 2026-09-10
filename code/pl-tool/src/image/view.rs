use schemars::JsonSchema;
use serde::Deserialize;
pub const TOOL_VIEW_IMAGE: &str = "view_image";
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewImageInput {
    /// Path to an image in the configured workspace.
    pub(crate) path: String,
}

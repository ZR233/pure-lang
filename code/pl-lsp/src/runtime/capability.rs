use serde::{Deserialize, Serialize};

/// 一个 workspace 当前 LSP server 的能力投影。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LspWorkspaceCapabilities {
    pub servers: Vec<LspWorkspaceServerCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspWorkspaceServerCapabilities {
    pub id: String,
    pub display_name: String,
    pub language_ids: Vec<String>,
    pub operations: Vec<String>,
    pub availability: String,
    pub ready: bool,
}

/// 描述一个可被 `lsp_query` 按 languageId 路由的语言。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageToolInfo {
    pub language_id: String,
    pub server_id: String,
    pub display_name: String,
    pub extensions: Vec<String>,
}

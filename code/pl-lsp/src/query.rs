//! LSP 语义查询、位置、诊断与结果合同。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDiagnostic {
    pub server_id: String,
    pub uri: String,
    pub path: String,
    pub range: LspRange,
    pub severity: Option<u32>,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
    pub received_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LspQueryOperation {
    GoToDefinition,
    FindReferences,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
    GoToImplementation,
    PrepareCallHierarchy,
    IncomingCalls,
    OutgoingCalls,
    Diagnostics,
}

impl LspQueryOperation {
    /// 全部受支持的查询操作；`lsp_capabilities` 与输入校验共用。
    pub fn all() -> &'static [Self] {
        &[
            Self::GoToDefinition,
            Self::FindReferences,
            Self::Hover,
            Self::DocumentSymbol,
            Self::WorkspaceSymbol,
            Self::GoToImplementation,
            Self::PrepareCallHierarchy,
            Self::IncomingCalls,
            Self::OutgoingCalls,
            Self::Diagnostics,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoToDefinition => "goToDefinition",
            Self::FindReferences => "findReferences",
            Self::Hover => "hover",
            Self::DocumentSymbol => "documentSymbol",
            Self::WorkspaceSymbol => "workspaceSymbol",
            Self::GoToImplementation => "goToImplementation",
            Self::PrepareCallHierarchy => "prepareCallHierarchy",
            Self::IncomingCalls => "incomingCalls",
            Self::OutgoingCalls => "outgoingCalls",
            Self::Diagnostics => "diagnostics",
        }
    }

    pub fn requires_position(self) -> bool {
        matches!(
            self,
            Self::GoToDefinition
                | Self::FindReferences
                | Self::Hover
                | Self::GoToImplementation
                | Self::PrepareCallHierarchy
                | Self::IncomingCalls
                | Self::OutgoingCalls
        )
    }

    pub fn requires_file(self) -> bool {
        self.requires_position() || matches!(self, Self::DocumentSymbol)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspQuery {
    pub operation: LspQueryOperation,
    pub file_path: Option<PathBuf>,
    pub line: Option<u32>,
    pub character: Option<u32>,
    pub query: Option<String>,
    pub max_results: Option<usize>,
    /// 显式指定目标语言 ID，优先于文件扩展名路由。
    #[serde(default)]
    pub language_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspQueryResult {
    pub success: bool,
    pub operation: LspQueryOperation,
    pub server_id: Option<String>,
    pub result: String,
    pub result_count: Option<usize>,
    pub file_count: Option<usize>,
}

impl LspQueryResult {
    pub fn unavailable(operation: LspQueryOperation, message: impl Into<String>) -> Self {
        Self {
            success: false,
            operation,
            server_id: None,
            result: message.into(),
            result_count: None,
            file_count: None,
        }
    }
}

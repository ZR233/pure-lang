use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub type LspResult<T> = Result<T, LspRuntimeError>;

#[derive(Debug, thiserror::Error)]
pub enum LspRuntimeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("LSP server error {code}: {message}")]
    Server { code: i64, message: String },
    #[error("LSP request timed out: {0}")]
    Timeout(String),
    #[error("LSP server unavailable: {0}")]
    Unavailable(String),
    #[error("invalid LSP query: {0}")]
    InvalidQuery(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LspAvailabilityKind {
    Checking,
    Available,
    Unavailable,
    MissingCommand,
    Disabled,
}

impl LspAvailabilityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::MissingCommand => "missingCommand",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LspActivityKind {
    Idle,
    Busy,
    Indexing,
}

impl LspActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Indexing => "indexing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerSnapshot {
    pub id: String,
    pub display_name: String,
    pub extensions: Vec<String>,
    pub language_ids: Vec<String>,
    pub availability_kind: LspAvailabilityKind,
    pub availability_message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub diagnostic_count: usize,
    pub activity_kind: LspActivityKind,
    pub activity_title: Option<String>,
    pub activity_message: Option<String>,
    pub activity_percentage: Option<u32>,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// 描述一个可注册为工具的语言 LSP 信息。
///
/// 每个 `LanguageToolInfo` 对应一个当前处于 `Available` 状态的 LSP 服务器所支持的某一种语言。
/// `pl-core` 使用此信息为每个语言生成独立的 LSP 查询工具（如 `lsp_query_rust`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageToolInfo {
    /// LSP 语言标识符，如 "rust"、"typescript"。
    pub language_id: String,
    /// LSP 服务器标识，如 "rust-analyzer"。
    pub server_id: String,
    /// 显示名称，如 "rust-analyzer"。
    pub display_name: String,
    /// 该语言关联的文件扩展名，如 [".rs"]。
    pub extensions: Vec<String>,
}

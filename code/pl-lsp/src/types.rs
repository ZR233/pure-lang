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
    #[error("LSP routing failed: {0}")]
    Routing(#[from] LspRoutingError),
}

/// languageId 路由的 typed 拒绝。
///
/// 同一 language id 被多个 server 声明且都匹配当前 workspace 时不按注册顺序猜测，
/// 由 `lsp_query` 把候选列表作为可恢复错误返回给模型。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LspRoutingError {
    #[error("ambiguous language `{language_id}`: multiple LSP servers declare it: {servers:?}")]
    AmbiguousLanguage {
        language_id: String,
        servers: Vec<String>,
    },
    #[error("ambiguous file extension `{extension}`: multiple LSP servers declare it: {servers:?}")]
    AmbiguousPath {
        extension: String,
        servers: Vec<String>,
    },
}

/// server 组件缺失的 typed 描述：由 driver 探测产生，repair 消费。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspMissingComponent {
    /// 缺失组件的标签，如 rustup 的 `rust-analyzer` component 名。
    pub component: String,
    /// driver 给出的修复说明（展示给用户，repair 按组件执行）。
    pub repair_hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LspAvailabilityKind {
    Checking,
    Available,
    Unavailable,
    MissingCommand,
    MissingServerComponent(LspMissingComponent),
    Disabled,
}

/// LSP reset 的明确作用域。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspScope {
    Server {
        workspace_root: PathBuf,
        server_id: String,
    },
    Workspace {
        workspace_root: PathBuf,
    },
    All,
}

impl LspAvailabilityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::MissingCommand => "missingCommand",
            Self::MissingServerComponent(_) => "missingServerComponent",
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

/// 一个 workspace 当前 LSP server 的能力投影。
///
/// 由 `LspRuntimeRegistry::capabilities_for_workspace` 生成；`lsp_capabilities`
/// 工具把它直接返回给模型，帮助其按 languageId 路由 `lsp_query`。
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
    /// 该 server 支持的 `lsp_query` 操作名（definition/references/...）。
    pub operations: Vec<String>,
    /// availability 标签（available/unavailable/...），仅用于诊断展示。
    pub availability: String,
    /// server 是否处于可查询状态。
    pub ready: bool,
}

/// 描述一个可被 `lsp_query` 按 languageId 路由的语言。
///
/// 每个 `LanguageToolInfo` 对应一个当前处于 `Available` 状态的 LSP 服务器所支持的某一种语言。
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

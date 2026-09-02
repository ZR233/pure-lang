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

/// languageId 与文件扩展名路由的 typed 拒绝。
///
/// 同一目标被多个 server 声明且都匹配当前 workspace 时不按注册顺序猜测，
/// 而是把候选列表作为可恢复错误返回给调用方。
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

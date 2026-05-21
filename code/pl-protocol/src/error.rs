use thiserror::Error;

#[derive(Debug, Error)]
pub enum PureError {
    #[error("LLM provider error: {0}")]
    LlmError(String),

    #[error("context window exceeded: {0} tokens")]
    ContextOverflow(usize),

    #[error("tool not found: {0}")]
    ToolNotFound(String),

    #[error("tool execution failed: {tool}: {error}")]
    ToolExecutionFailed { tool: String, error: String },

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("sandbox error: {0}")]
    SandboxError(String),

    #[error("memory store error: {0}")]
    MemoryError(String),

    #[error("configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    HttpError(String),
}

pub type Result<T> = std::result::Result<T, PureError>;

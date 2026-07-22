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

    #[error("agent limit reached: max agents {max_agents}")]
    AgentLimitReached { max_agents: usize },

    #[error("agent depth limit reached: max depth {max_depth}")]
    AgentDepthLimitReached { max_depth: u32 },

    #[error("provider capacity unavailable: {message}")]
    ProviderCapacity { message: String },

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

    #[error("transient model transport error: {message}")]
    TransientModelTransport {
        message: String,
        retry_after_ms: Option<u64>,
        code: Option<String>,
        http_status: Option<u16>,
    },
}

impl PureError {
    /// 构造一个可以安全重放完整模型请求的临时传输错误。
    pub fn transient_model_transport(message: impl Into<String>) -> Self {
        Self::TransientModelTransport {
            message: message.into(),
            retry_after_ms: None,
            code: None,
            http_status: None,
        }
    }

    /// 构造携带供应商建议等待时间的临时传输错误。
    pub fn transient_model_transport_after(
        message: impl Into<String>,
        retry_after_ms: u64,
    ) -> Self {
        Self::TransientModelTransport {
            message: message.into(),
            retry_after_ms: Some(retry_after_ms),
            code: None,
            http_status: None,
        }
    }

    /// 构造保留 provider code 与 HTTP 状态的临时模型错误。
    pub fn transient_model_failure(
        message: impl Into<String>,
        retry_after_ms: Option<u64>,
        code: Option<String>,
        http_status: Option<u16>,
    ) -> Self {
        Self::TransientModelTransport {
            message: message.into(),
            retry_after_ms,
            code,
            http_status,
        }
    }

    /// 返回该错误是否允许在工具尚未执行时重放完整模型请求。
    pub fn is_transient_model_transport(&self) -> bool {
        matches!(self, Self::TransientModelTransport { .. })
    }

    /// 返回供应商建议的重试等待时间。
    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            Self::TransientModelTransport { retry_after_ms, .. } => *retry_after_ms,
            Self::LlmError(_)
            | Self::ContextOverflow(_)
            | Self::ToolNotFound(_)
            | Self::ToolExecutionFailed { .. }
            | Self::AgentLimitReached { .. }
            | Self::AgentDepthLimitReached { .. }
            | Self::ProviderCapacity { .. }
            | Self::PermissionDenied(_)
            | Self::SandboxError(_)
            | Self::MemoryError(_)
            | Self::ConfigError(_)
            | Self::Io(_)
            | Self::SerdeJson(_)
            | Self::HttpError(_) => None,
        }
    }

    /// 返回瞬态模型错误携带的 provider code 与 HTTP 状态。
    pub fn transient_model_metadata(&self) -> Option<(Option<&str>, Option<u16>)> {
        match self {
            Self::TransientModelTransport {
                code, http_status, ..
            } => Some((code.as_deref(), *http_status)),
            Self::LlmError(_)
            | Self::ContextOverflow(_)
            | Self::ToolNotFound(_)
            | Self::ToolExecutionFailed { .. }
            | Self::AgentLimitReached { .. }
            | Self::AgentDepthLimitReached { .. }
            | Self::ProviderCapacity { .. }
            | Self::PermissionDenied(_)
            | Self::SandboxError(_)
            | Self::MemoryError(_)
            | Self::ConfigError(_)
            | Self::Io(_)
            | Self::SerdeJson(_)
            | Self::HttpError(_) => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, PureError>;

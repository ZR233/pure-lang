use serde::{Deserialize, Serialize};

/// Provider 失败的稳定语义类别；控制流不得从 message 或 provider code 反向推断。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderFailureKind {
    Authentication,
    Authorization,
    Capacity,
    Configuration,
    Transport,
    Protocol,
    Unknown,
}

/// Turn 失败所属的稳定领域类别。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnFailureCategory {
    Provider,
    ProviderCapacity,
    Tool,
    Validation,
    Protocol,
    Internal,
}

/// 宿主调度器应如何处理一次 turn 失败。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RetryDisposition {
    Retryable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<u64>,
    },
    Permanent,
}

impl RetryDisposition {
    /// 返回失败是否允许由宿主在新的 turn 中继续执行。
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    /// 返回 provider 建议的最短等待时间。
    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            Self::Retryable { retry_after_ms } => *retry_after_ms,
            Self::Permanent => None,
        }
    }
}

/// 跨 runtime 与宿主边界保存的结构化 turn 失败。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnFailure {
    pub category: TurnFailureCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_kind: Option<ProviderFailureKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    pub message: String,
    pub retry: RetryDisposition,
}

impl TurnFailure {
    /// 构造一个不应由宿主自动重试的失败。
    pub fn permanent(category: TurnFailureCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            provider_kind: None,
            code: None,
            http_status: None,
            message: message.into(),
            retry: RetryDisposition::Permanent,
        }
    }
}

/// Provider adapter 跨 crate 返回的结构化失败。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFailure {
    pub kind: ProviderFailureKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    pub message: String,
    pub retry: RetryDisposition,
}

impl std::fmt::Display for ProviderFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

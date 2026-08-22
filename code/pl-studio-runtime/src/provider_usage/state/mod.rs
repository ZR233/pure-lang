//! 单个 provider usage 的精确状态 payload。

mod failed;
mod missing_credential;
mod ready;
mod unsupported;

pub use failed::FailedProviderUsage;
pub use missing_credential::MissingCredentialProviderUsage;
pub use ready::ReadyProviderUsage;
pub use unsupported::UnsupportedProviderUsage;

use serde::{Deserialize, Serialize};

use super::ProviderUsageData;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ProviderUsageState {
    Unsupported(UnsupportedProviderUsage),
    MissingCredential(MissingCredentialProviderUsage),
    Ready(ReadyProviderUsage),
    Failed(FailedProviderUsage),
}

impl ProviderUsageState {
    pub fn unsupported() -> Self {
        Self::Unsupported(UnsupportedProviderUsage)
    }

    pub fn missing_credential(message: impl Into<String>) -> Self {
        Self::MissingCredential(MissingCredentialProviderUsage::new(message))
    }

    pub fn ready(data: ProviderUsageData) -> Self {
        Self::Ready(ReadyProviderUsage::new(data))
    }

    pub fn failed(error: pl_protocol::StateError) -> Self {
        Self::Failed(FailedProviderUsage::new(error))
    }
}

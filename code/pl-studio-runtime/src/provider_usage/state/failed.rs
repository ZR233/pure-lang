use pl_protocol::StateError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FailedProviderUsage {
    error: StateError,
}

impl FailedProviderUsage {
    pub fn new(error: StateError) -> Self {
        Self { error }
    }

    pub fn error(&self) -> &StateError {
        &self.error
    }
}

use serde::{Deserialize, Serialize};

use super::super::ProviderUsageData;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadyProviderUsage {
    data: ProviderUsageData,
}

impl ReadyProviderUsage {
    pub fn new(data: ProviderUsageData) -> Self {
        Self { data }
    }

    pub fn data(&self) -> &ProviderUsageData {
        &self.data
    }

    pub fn into_data(self) -> ProviderUsageData {
        self.data
    }
}

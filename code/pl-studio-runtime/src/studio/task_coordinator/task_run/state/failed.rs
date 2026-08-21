use serde::{Deserialize, Serialize};

use super::DesignProgress;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailedState {
    generation: u64,
    design: DesignProgress,
    message: String,
    failure_id: Option<String>,
}

impl FailedState {
    pub(crate) fn new(
        design: DesignProgress,
        generation: u64,
        message: String,
        failure_id: Option<String>,
    ) -> Self {
        Self {
            generation,
            design,
            message,
            failure_id,
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn design(&self) -> &DesignProgress {
        &self.design
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn failure_id(&self) -> Option<&str> {
        self.failure_id.as_deref()
    }
}

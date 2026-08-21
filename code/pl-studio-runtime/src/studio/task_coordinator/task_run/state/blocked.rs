use serde::{Deserialize, Serialize};

use super::{BlockedRecovery, DesignProgress};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlockedState {
    generation: u64,
    design: DesignProgress,
    message: String,
    recovery: BlockedRecovery,
}

impl BlockedState {
    pub(crate) fn new(
        design: DesignProgress,
        generation: u64,
        message: String,
        recovery: BlockedRecovery,
    ) -> Self {
        Self {
            generation,
            design,
            message,
            recovery,
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

    pub(crate) const fn recovery(&self) -> &BlockedRecovery {
        &self.recovery
    }
}

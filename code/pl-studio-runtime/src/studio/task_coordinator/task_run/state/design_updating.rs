use serde::{Deserialize, Serialize};

use super::DesignProgress;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesignUpdatingState {
    generation: u64,
    design: DesignProgress,
}

impl DesignUpdatingState {
    pub(crate) const fn new() -> Self {
        Self {
            generation: 0,
            design: DesignProgress::Updating,
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn design(&self) -> &DesignProgress {
        &self.design
    }
}

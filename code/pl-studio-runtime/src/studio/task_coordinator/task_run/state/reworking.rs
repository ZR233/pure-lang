use serde::{Deserialize, Serialize};

use super::{DesignProgress, FinalizedDesign};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReworkingState {
    generation: u64,
    design: DesignProgress,
    status_message: String,
}

impl ReworkingState {
    pub(crate) fn new(design: FinalizedDesign, generation: u64, status_message: String) -> Self {
        Self {
            generation,
            design: DesignProgress::from_finalized(design),
            status_message,
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn design(&self) -> &DesignProgress {
        &self.design
    }

    pub(crate) fn status_message(&self) -> &str {
        &self.status_message
    }
}

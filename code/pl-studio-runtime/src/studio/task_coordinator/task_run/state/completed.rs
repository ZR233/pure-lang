use serde::{Deserialize, Serialize};

use super::{DesignProgress, FinalizedDesign};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompletedState {
    generation: u64,
    design: DesignProgress,
}

impl CompletedState {
    pub(crate) fn new(design: FinalizedDesign, generation: u64) -> Self {
        Self {
            generation,
            design: DesignProgress::from_finalized(design),
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn design(&self) -> &DesignProgress {
        &self.design
    }
}

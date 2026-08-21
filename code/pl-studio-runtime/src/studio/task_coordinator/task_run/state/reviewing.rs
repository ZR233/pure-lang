use serde::{Deserialize, Serialize};

use super::{DesignProgress, FinalizedDesign, ReviewTarget};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewingState {
    generation: u64,
    design: DesignProgress,
    target: ReviewTarget,
    status_message: Option<String>,
}

impl ReviewingState {
    pub(crate) fn new(design: FinalizedDesign, generation: u64, target: ReviewTarget) -> Self {
        Self {
            generation,
            design: DesignProgress::from_finalized(design),
            target,
            status_message: Some("integrated review is required".to_string()),
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn design(&self) -> &DesignProgress {
        &self.design
    }

    pub(crate) fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    pub(crate) const fn target(&self) -> &ReviewTarget {
        &self.target
    }
}

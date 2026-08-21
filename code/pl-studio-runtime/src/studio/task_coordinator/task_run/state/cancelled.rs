use serde::{Deserialize, Serialize};

use super::{DesignProgress, TaskStopRequest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CancelledState {
    generation: u64,
    design: DesignProgress,
    message: String,
    request: Option<TaskStopRequest>,
}

impl CancelledState {
    pub(crate) fn new(
        design: DesignProgress,
        generation: u64,
        message: String,
        request: Option<TaskStopRequest>,
    ) -> Self {
        Self {
            generation,
            design,
            message,
            request,
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

    pub(crate) const fn request(&self) -> Option<&TaskStopRequest> {
        self.request.as_ref()
    }
}

use serde::{Deserialize, Serialize};

use super::{DesignProgress, TaskStopRequest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoppingState {
    generation: u64,
    design: DesignProgress,
    request: TaskStopRequest,
    status_message: String,
}

impl StoppingState {
    pub(crate) fn new(design: DesignProgress, generation: u64, request: TaskStopRequest) -> Self {
        Self {
            generation,
            design,
            request,
            status_message: "task stop is settling agents".to_string(),
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn design(&self) -> &DesignProgress {
        &self.design
    }

    pub(crate) const fn request(&self) -> &TaskStopRequest {
        &self.request
    }

    pub(crate) fn status_message(&self) -> &str {
        &self.status_message
    }
}

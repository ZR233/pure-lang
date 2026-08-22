use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PassedReview {
    reviewer_thread_id: String,
    summary: String,
}

impl PassedReview {
    pub(crate) fn new(reviewer_thread_id: String, summary: String) -> Self {
        Self {
            reviewer_thread_id,
            summary,
        }
    }
    pub(crate) fn reviewer_thread_id(&self) -> &str {
        &self.reviewer_thread_id
    }
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }
}

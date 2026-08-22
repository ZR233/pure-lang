use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CancelledReview {
    reviewer_thread_id: Option<String>,
    reason: String,
    summary: String,
}

impl CancelledReview {
    pub(crate) fn new(reviewer_thread_id: Option<String>, reason: String, summary: String) -> Self {
        Self {
            reviewer_thread_id,
            reason,
            summary,
        }
    }
    pub(crate) fn reviewer_thread_id(&self) -> Option<&str> {
        self.reviewer_thread_id.as_deref()
    }
    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailedReview {
    reviewer_thread_id: Option<String>,
    error: String,
    summary: String,
}

impl FailedReview {
    pub(crate) fn new(reviewer_thread_id: Option<String>, error: String, summary: String) -> Self {
        Self {
            reviewer_thread_id,
            error,
            summary,
        }
    }
    pub(crate) fn reviewer_thread_id(&self) -> Option<&str> {
        self.reviewer_thread_id.as_deref()
    }
    pub(crate) fn error(&self) -> &str {
        &self.error
    }
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }
}

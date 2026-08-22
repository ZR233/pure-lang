use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DispatchedReview {
    reviewer_thread_id: String,
}

impl DispatchedReview {
    pub(crate) fn new(reviewer_thread_id: String) -> Self {
        Self { reviewer_thread_id }
    }
    pub(crate) fn reviewer_thread_id(&self) -> &str {
        &self.reviewer_thread_id
    }
}

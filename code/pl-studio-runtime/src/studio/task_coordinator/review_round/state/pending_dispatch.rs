use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingReviewDispatch {}

impl PendingReviewDispatch {
    pub(crate) const fn new() -> Self {
        Self {}
    }
}

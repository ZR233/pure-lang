use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangesRequiredWorkUnit {
    pub(super) completion_id: String,
    pub(super) completion_revision: u32,
    pub(super) review_round_id: String,
    pub(super) continuation_revision: u64,
    pub(super) slice_count: u32,
}

impl ChangesRequiredWorkUnit {
    pub(crate) fn completion_id(&self) -> &str {
        &self.completion_id
    }

    pub(crate) const fn completion_revision(&self) -> u32 {
        self.completion_revision
    }

    pub(crate) fn review_round_id(&self) -> &str {
        &self.review_round_id
    }

    pub(crate) const fn continuation_revision(&self) -> u64 {
        self.continuation_revision
    }

    pub(crate) const fn slice_count(&self) -> u32 {
        self.slice_count
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReviewPassedOutcome {
    Delivery,
    NoDelivery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewPassedWorkUnit {
    pub(super) completion_id: String,
    pub(super) completion_revision: u32,
    pub(super) review_round_id: String,
    pub(super) outcome: ReviewPassedOutcome,
    pub(super) verification_summary: String,
}

impl ReviewPassedWorkUnit {
    pub(crate) fn completion_id(&self) -> &str {
        &self.completion_id
    }

    pub(crate) const fn completion_revision(&self) -> u32 {
        self.completion_revision
    }

    pub(crate) fn review_round_id(&self) -> &str {
        &self.review_round_id
    }

    pub(crate) const fn outcome(&self) -> ReviewPassedOutcome {
        self.outcome
    }

    pub(crate) fn verification_summary(&self) -> &str {
        &self.verification_summary
    }
}

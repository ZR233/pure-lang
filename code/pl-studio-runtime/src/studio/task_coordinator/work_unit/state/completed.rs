use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum WorkUnitCompletionOutcome {
    Merged { merge_record_id: String },
    NoDelivery { completion_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CompletedWorkUnit {
    pub(super) outcome: WorkUnitCompletionOutcome,
}

impl CompletedWorkUnit {
    pub(crate) const fn outcome(&self) -> &WorkUnitCompletionOutcome {
        &self.outcome
    }
}

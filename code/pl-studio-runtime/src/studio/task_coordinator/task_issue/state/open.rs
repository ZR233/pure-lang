use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTaskIssue {
    failure: pl_protocol::TurnFailure,
}

impl OpenTaskIssue {
    pub(crate) fn new(failure: pl_protocol::TurnFailure) -> Self {
        Self { failure }
    }

    pub(crate) fn failure(&self) -> &pl_protocol::TurnFailure {
        &self.failure
    }
}

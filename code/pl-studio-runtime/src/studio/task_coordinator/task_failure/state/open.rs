use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenTaskFailure {
    failure: pl_protocol::TurnFailure,
}

impl OpenTaskFailure {
    pub(crate) fn new(failure: pl_protocol::TurnFailure) -> Self {
        Self { failure }
    }

    pub(crate) fn failure(&self) -> &pl_protocol::TurnFailure {
        &self.failure
    }
}

use serde::{Deserialize, Serialize};

use super::CommandTerminationReason;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminatingCommandProcess {
    reason: CommandTerminationReason,
}

impl TerminatingCommandProcess {
    pub(super) fn new(reason: CommandTerminationReason) -> Self {
        Self { reason }
    }

    pub fn reason(&self) -> CommandTerminationReason {
        self.reason
    }
}

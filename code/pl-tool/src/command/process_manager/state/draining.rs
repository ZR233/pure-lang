use serde::{Deserialize, Serialize};

use super::{CommandProcessFailure, CommandProcessFinalResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DrainingCommandProcess {
    result: CommandProcessFinalResult,
}

impl DrainingCommandProcess {
    pub(super) fn new(result: CommandProcessFinalResult) -> Self {
        Self { result }
    }

    pub fn result(&self) -> &CommandProcessFinalResult {
        &self.result
    }

    pub(super) fn record_output_error(&mut self, message: String) {
        if let CommandProcessFinalResult::Succeeded { exit_code } = self.result {
            self.result = CommandProcessFinalResult::Failed {
                failure: CommandProcessFailure::Output {
                    message,
                    exit_code: Some(exit_code),
                },
            };
        }
    }
}

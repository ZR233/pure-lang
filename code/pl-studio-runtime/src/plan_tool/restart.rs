use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
pub const TOOL_PLAN_RESTART: &str = "plan_restart";
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRestartInput {
    /// CAS revision returned by plan_current.
    pub(super) expected_revision: u64,
    /// Why the current Plan lifecycle must be discarded.
    pub(super) reason: String,
}

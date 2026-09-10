use super::machine::RestartInput;
use pl_protocol::PureError;
pub const TOOL_WORKFLOW_RESTART: &str = "workflow_restart";
/// Validates the same typed input accepted by the dynamic workflow tool.
pub fn validate_workflow_restart_arguments(arguments: serde_json::Value) -> Result<(), PureError> {
    serde_json::from_value::<RestartInput>(arguments)
        .map(|_| ())
        .map_err(PureError::from)
}

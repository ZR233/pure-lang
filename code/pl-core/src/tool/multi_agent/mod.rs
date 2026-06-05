mod events;
mod runner;
mod tools;
mod types;

#[cfg(test)]
mod tests;

use pl_protocol::PureError;
use serde::Serialize;

use crate::config::ModelRole;

use super::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

pub use types::{
    CloseAgentTool, FollowupTaskTool, ListAgentsTool, SendMessageTool, SpawnAgentTool,
    WaitAgentTool,
};

pub(super) use events::emit_agent_record;
pub(super) use runner::run_agent_turn;
pub(super) use types::AgentRunConfig;

fn role_key(role: Option<&str>) -> Result<String, PureError> {
    let role = role
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("executor");
    ModelRole::from_key(role)
        .map(|role| role.key().to_string())
        .ok_or_else(|| PureError::ToolExecutionFailed {
            tool: "spawn_agent".to_string(),
            error: format!("unsupported agentType: {role}"),
        })
}

fn message_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Agent id, relative path, or canonical path."
            },
            "message": {
                "type": "string",
                "description": "Message to send to the target agent."
            }
        },
        "required": ["target", "message"]
    })
}

fn invalid_spawn_input(error: serde_json::Error) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "spawn_agent".to_string(),
        error: format!("invalid input: {error}"),
    }
}

fn json_output(value: impl Serialize) -> Result<ToolOutput, PureError> {
    let description =
        serde_json::to_string(&value).map_err(|error| PureError::ToolExecutionFailed {
            tool: "agent".to_string(),
            error: format!("failed to serialize output: {error}"),
        })?;
    Ok(ToolOutput {
        description,
        truncated: super::truncation::OutputTruncation::empty(),
        output_file: std::path::PathBuf::new(),
        exit_code: None,
        timed_out: false,
    })
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(super) fn current_agent_path(context: &ToolContext) -> String {
    context
        .active_subagent
        .as_ref()
        .and_then(|subagent| subagent.agent_path.clone())
        .unwrap_or_else(|| crate::AgentPath::ROOT.to_string())
}

pub(super) fn child_agent_options(options: &crate::TurnOptions) -> crate::TurnOptions {
    use tokio_util::sync::CancellationToken;
    let token = options
        .cancellation_token
        .as_ref()
        .map(CancellationToken::child_token)
        .unwrap_or_default();
    options.clone().with_cancellation(token)
}

mod host;
mod schema;
mod tools;
mod types;

#[cfg(test)]
mod tests;

use pl_protocol::PureError;
use pl_protocol::{Message, MessageRole};
use serde::Serialize;

use crate::config::ModelRole;
use crate::session::CoreSession;

use super::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};
use crate::agent::AgentRecord;
use types::{AgentToolRecord, CompactAgentRecord};

pub use host::{
    AgentControlAgentRecord, AgentControlAgentType, AgentControlAgentTypePolicy,
    AgentControlBackend, AgentControlListOutput, AgentControlListRequest,
    AgentControlMessageOutput, AgentControlPolicy, AgentControlSendInputOutput,
    AgentControlSendInputRequest, AgentControlSpawnOutput, AgentControlSpawnRequest,
    AgentControlStatusKind, AgentControlTargetRequest, AgentControlTool, AgentControlWaitOutput,
    AgentControlWaitRequest, AllowAllAgentControlPolicy,
};
pub use schema::{
    AgentControlToolKind, TOOL_CLOSE_AGENT, TOOL_LIST_AGENTS, TOOL_RESUME_AGENT, TOOL_SEND_INPUT,
    TOOL_SPAWN_AGENT, TOOL_WAIT_AGENT,
};
pub use types::{
    CloseAgentTool, ListAgentsTool, ResumeAgentTool, SendInputTool, SpawnAgentTool, WaitAgentTool,
};

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

fn invalid_spawn_input(error: serde_json::Error) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "spawn_agent".to_string(),
        error: format!("invalid input: {error}"),
    }
}

fn validate_spawn_authority(context: &ToolContext, requested_role: &str) -> Result<(), PureError> {
    if context.active_subagent.is_some() {
        return Err(PureError::ToolExecutionFailed {
            tool: "spawn_agent".to_string(),
            error: "only the root owner may spawn agents".to_string(),
        });
    }
    match context.mode {
        crate::CompileMode::Simple if requested_role != "explorer" => {
            Err(PureError::ToolExecutionFailed {
                tool: "spawn_agent".to_string(),
                error: "simple mode may only spawn explorer agents".to_string(),
            })
        }
        crate::CompileMode::Task if requested_role == "planner" => {
            Err(PureError::ToolExecutionFailed {
                tool: "spawn_agent".to_string(),
                error: "task planner cannot spawn another planner".to_string(),
            })
        }
        crate::CompileMode::Simple | crate::CompileMode::Task => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForkTurns {
    None,
    All,
    Last(usize),
}

impl ForkTurns {
    fn parse(value: Option<&str>) -> Result<Self, PureError> {
        let value = value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("none");
        if value.eq_ignore_ascii_case("none") {
            return Ok(Self::None);
        }
        if value.eq_ignore_ascii_case("all") {
            return Ok(Self::All);
        }
        let turns = value
            .parse::<usize>()
            .map_err(|_| PureError::ToolExecutionFailed {
                tool: "spawn_agent".to_string(),
                error: "forkTurns must be `none`, `all`, or a positive integer string".to_string(),
            })?;
        if turns == 0 {
            return Err(PureError::ToolExecutionFailed {
                tool: "spawn_agent".to_string(),
                error: "forkTurns must be `none`, `all`, or a positive integer string".to_string(),
            });
        }
        Ok(Self::Last(turns))
    }
}

fn fork_session(parent: &CoreSession, mode: ForkTurns) -> CoreSession {
    match mode {
        ForkTurns::None => CoreSession::new(),
        ForkTurns::All => CoreSession::from_messages(filtered_parent_messages(parent.messages())),
        ForkTurns::Last(turns) => CoreSession::from_messages(last_user_turns(
            filtered_parent_messages(parent.messages()),
            turns,
        )),
    }
}

fn filtered_parent_messages(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter_map(|message| match message.role {
            MessageRole::System | MessageRole::User => Some(filtered_message(message)),
            MessageRole::Assistant if !message.metadata.contains_key("tool_calls") => {
                Some(filtered_message(message))
            }
            MessageRole::Assistant | MessageRole::Tool => None,
        })
        .collect()
}

fn filtered_message(message: &Message) -> Message {
    Message {
        role: message.role,
        content: message.content.clone(),
        reasoning_content: None,
        metadata: std::collections::HashMap::new(),
    }
}

fn last_user_turns(messages: Vec<Message>, turns: usize) -> Vec<Message> {
    let (system_messages, conversation): (Vec<_>, Vec<_>) = messages
        .into_iter()
        .partition(|message| message.role == MessageRole::System);
    let mut start = 0;
    let mut seen = 0;
    for (index, message) in conversation.iter().enumerate().rev() {
        if message.role == MessageRole::User {
            seen += 1;
            if seen == turns {
                start = index;
                break;
            }
        }
    }
    let mut result = system_messages;
    result.extend(conversation.into_iter().skip(start));
    result
}

fn agent_tool_records(agents: &[AgentRecord]) -> Vec<AgentToolRecord> {
    agents
        .iter()
        .cloned()
        .map(|agent| CompactAgentRecord {
            path: agent.path,
            status: agent.status,
            role: agent.role,
            task: crate::core::compact_text(&agent.task),
            summary: agent
                .summary
                .map(|summary| crate::core::compact_text(&summary)),
            error: agent.error.map(|error| crate::core::compact_text(&error)),
        })
        .collect()
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
        runtime_events: Vec::new(),
    })
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

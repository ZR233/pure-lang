use pl_model::ToolSchema;
use serde_json::{Value, json};

pub const TOOL_SPAWN_AGENT: &str = "spawn_agent";
pub const TOOL_SEND_INPUT: &str = "send_input";
pub const TOOL_WAIT_AGENT: &str = "wait_agent";
pub const TOOL_LIST_AGENTS: &str = "list_agents";
pub const TOOL_CLOSE_AGENT: &str = "close_agent";
pub const TOOL_RESUME_AGENT: &str = "resume_agent";

/// 共享 agent 控制工具的模型可见协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentControlToolKind {
    SpawnAgent,
    SendInput,
    WaitAgent,
    ListAgents,
    CloseAgent,
    ResumeAgent,
}

impl AgentControlToolKind {
    pub fn all() -> &'static [Self] {
        &[
            Self::SpawnAgent,
            Self::SendInput,
            Self::WaitAgent,
            Self::ListAgents,
            Self::CloseAgent,
            Self::ResumeAgent,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::SpawnAgent => TOOL_SPAWN_AGENT,
            Self::SendInput => TOOL_SEND_INPUT,
            Self::WaitAgent => TOOL_WAIT_AGENT,
            Self::ListAgents => TOOL_LIST_AGENTS,
            Self::CloseAgent => TOOL_CLOSE_AGENT,
            Self::ResumeAgent => TOOL_RESUME_AGENT,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::SpawnAgent => {
                "Spawn a managed sub-agent for an independent task. The spawned agent runs asynchronously; use wait_agent to observe completion."
            }
            Self::SendInput => {
                "Send input to an existing agent. Defaults to queueing the input; set triggerTurn=true to start a new turn for a waiting child agent."
            }
            Self::WaitAgent => {
                "Wait for managed sub-agent activity or completion. Timeout is a normal pending result, not task failure."
            }
            Self::ListAgents => "List known managed sub-agents in the current collaboration tree.",
            Self::CloseAgent => {
                "Close an existing managed sub-agent. The root agent cannot be closed."
            }
            Self::ResumeAgent => "Resume a closed managed sub-agent.",
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::SpawnAgent => object_schema(vec![
                (
                    "taskName",
                    json!({
                        "type": "string",
                        "description": "Stable lowercase task name using letters, digits, and underscores."
                    }),
                    true,
                ),
                (
                    "message",
                    json!({
                        "type": "string",
                        "description": "Initial task message for the spawned agent."
                    }),
                    true,
                ),
                (
                    "agentType",
                    json!({
                        "type": "string",
                        "enum": ["explorer", "planner", "executor", "reviewer"],
                        "description": "Agent role. Defaults to executor."
                    }),
                    false,
                ),
                (
                    "forkTurns",
                    json!({
                        "type": "string",
                        "description": "Parent history to inherit: none, all, or a positive integer string. Defaults to none. Inherited history is filtered to remove tool calls/results and reasoning."
                    }),
                    false,
                ),
                (
                    "model",
                    json!({
                        "type": "string",
                        "description": "Optional model override; omitted to inherit parent policy."
                    }),
                    false,
                ),
                (
                    "reasoningEffort",
                    json!({
                        "type": "string",
                        "description": "Optional reasoning effort override."
                    }),
                    false,
                ),
            ]),
            Self::SendInput => object_schema(vec![
                (
                    "target",
                    json!({
                        "type": "string",
                        "description": "Agent id, relative path, or canonical path."
                    }),
                    true,
                ),
                (
                    "message",
                    json!({
                        "type": "string",
                        "description": "Message to send to the target agent."
                    }),
                    true,
                ),
                (
                    "triggerTurn",
                    json!({
                        "type": "boolean",
                        "description": "When true, trigger a new turn for a waiting child agent instead of only queueing the input."
                    }),
                    false,
                ),
                (
                    "interrupt",
                    json!({
                        "type": "boolean",
                        "description": "When true, interrupt a busy target before enqueueing this input if the host supports it."
                    }),
                    false,
                ),
            ]),
            Self::WaitAgent => object_schema(vec![(
                "timeoutMs",
                json!({
                    "type": "integer",
                    "minimum": 100,
                    "description": "Wait timeout in milliseconds. Defaults to 30000."
                }),
                false,
            )]),
            Self::ListAgents => object_schema(vec![(
                "pathPrefix",
                json!({
                    "type": "string",
                    "description": "Optional canonical path prefix, such as /root/research."
                }),
                false,
            )]),
            Self::CloseAgent | Self::ResumeAgent => object_schema(vec![(
                "target",
                json!({
                    "type": "string",
                    "description": "Agent id, relative path, or canonical path."
                }),
                true,
            )]),
        }
    }

    pub fn to_schema(self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

fn object_schema(properties: Vec<(&str, Value, bool)>) -> Value {
    let mut props = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, schema, is_required) in properties {
        props.insert(name.to_string(), schema);
        if is_required {
            required.push(name);
        }
    }
    let mut object = serde_json::Map::new();
    object.insert("type".to_string(), json!("object"));
    object.insert("properties".to_string(), Value::Object(props));
    if !required.is_empty() {
        object.insert("required".to_string(), json!(required));
    }
    Value::Object(object)
}

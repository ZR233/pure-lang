use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use pl_protocol::PureError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::{AgentAccessPolicy, AgentId, AgentSnapshot, AgentTargetSelector};
use super::{ForkTurns, TOOL_SPAWN_AGENT};
use crate::{AgentSession, AgentSessionForkPolicy, ToolOutput};

pub(super) fn fork_session(
    parent: &AgentSession,
    mode: ForkTurns,
) -> Result<AgentSession, PureError> {
    match mode {
        ForkTurns::None => Ok(parent.fork(AgentSessionForkPolicy::Empty)),
        ForkTurns::All => Ok(parent.fork(AgentSessionForkPolicy::AllMessages)),
        ForkTurns::Last(0) => Err(tool_error(
            TOOL_SPAWN_AGENT,
            "forkTurns.last must be greater than zero".to_string(),
        )),
        ForkTurns::Last(turns) => Ok(parent.fork(AgentSessionForkPolicy::LastUserTurns(
            NonZeroUsize::new(turns).expect("non-zero fork turn count was validated"),
        ))),
    }
}

pub(super) fn filter_visible(
    snapshots: &[AgentSnapshot],
    caller: &AgentId,
    selector: &AgentTargetSelector,
) -> Vec<AgentSnapshot> {
    match selector {
        AgentTargetSelector::None => Vec::new(),
        AgentTargetSelector::All => snapshots.to_vec(),
        AgentTargetSelector::Explicit(ids) => snapshots
            .iter()
            .filter(|snapshot| ids.contains(&snapshot.identity.id))
            .cloned()
            .collect(),
        AgentTargetSelector::Tree => {
            let parents = parent_map(snapshots);
            let caller_root = root_id(caller, &parents);
            snapshots
                .iter()
                .filter(|snapshot| root_id(&snapshot.identity.id, &parents) == caller_root)
                .cloned()
                .collect()
        }
    }
}

pub(super) fn agent_path(id: &AgentId, snapshots: &[AgentSnapshot]) -> Vec<AgentId> {
    let parents = parent_map(snapshots);
    let mut path = vec![id.clone()];
    let mut current = id.clone();
    let mut remaining = parents.len();
    while remaining > 0 {
        let Some(Some(parent)) = parents.get(&current) else {
            break;
        };
        path.push(parent.clone());
        current = parent.clone();
        remaining -= 1;
    }
    path.reverse();
    path
}

fn parent_map(snapshots: &[AgentSnapshot]) -> BTreeMap<AgentId, Option<AgentId>> {
    snapshots
        .iter()
        .map(|snapshot| {
            (
                snapshot.identity.id.clone(),
                snapshot.identity.parent_id.clone(),
            )
        })
        .collect()
}

fn root_id(id: &AgentId, parents: &BTreeMap<AgentId, Option<AgentId>>) -> AgentId {
    let mut current = id.clone();
    let mut remaining = parents.len();
    while remaining > 0 {
        let Some(Some(parent)) = parents.get(&current) else {
            break;
        };
        current = parent.clone();
        remaining -= 1;
    }
    current
}

pub(super) fn spawn_schema(policy: &AgentAccessPolicy) -> Value {
    let roles = policy
        .spawn_roles
        .iter()
        .map(|role| Value::String(role.to_string()))
        .collect::<Vec<_>>();
    object_schema(vec![
        ("message", json!({ "type": "string" }), true),
        ("role", json!({ "type": "string", "enum": roles }), true),
        (
            "forkTurns",
            json!({
                "oneOf": [
                    { "const": "none" },
                    { "const": "all" },
                    {
                        "type": "object",
                        "properties": { "last": { "type": "integer", "minimum": 1 } },
                        "required": ["last"],
                        "additionalProperties": false
                    }
                ]
            }),
            false,
        ),
        ("metadata", json!({ "type": "object" }), false),
    ])
}

pub(super) fn progress_schema() -> Value {
    object_schema(vec![
        (
            "stage",
            json!({
                "type": "string",
                "enum": [
                    "exploring",
                    "implementing",
                    "verifying",
                    "blocked",
                    "readyForCompletion"
                ]
            }),
            true,
        ),
        (
            "summary",
            json!({ "type": "string", "maxLength": 1200 }),
            true,
        ),
        (
            "nextStep",
            json!({ "type": "string", "maxLength": 500 }),
            true,
        ),
        (
            "detail",
            json!({
                "type": "string",
                "maxLength": 20000,
                "description": "Optional substantive report content appended to the durable submission log and read in full by the orchestrator via read_agent_submissions."
            }),
            false,
        ),
    ])
}

/// send_message 的 schema：target 是运行时校验的直接子代理，因此这里只给字符串类型。
pub(super) fn send_message_schema() -> Value {
    object_schema(vec![
        (
            "target",
            json!({
                "type": "string",
                "description": "Id of a direct child agent to steer. Only parent-to-direct-child insertion is allowed."
            }),
            true,
        ),
        ("message", json!({ "type": "string" }), true),
    ])
}

pub(super) fn submissions_schema(selector: &AgentTargetSelector) -> Value {
    object_schema(vec![
        (
            "target",
            target_property_schema(
                selector,
                Some("Agent id whose durable stage submission history should be read."),
            ),
            true,
        ),
        (
            "offset",
            json!({
                "type": "integer",
                "minimum": 0,
                "default": 0,
                "description": "Zero-based offset into the submission history."
            }),
            false,
        ),
        (
            "limit",
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": 50,
                "default": 20,
                "description": "Maximum number of submissions to return in this page."
            }),
            false,
        ),
    ])
}

pub(super) fn wait_schema(selector: &AgentTargetSelector) -> Value {
    object_schema(vec![(
        "targets",
        json!({
            "type": "array",
            "items": target_property_schema(
                selector,
                Some("Agent id whose next progress, interaction, or terminal change should end the wait.")
            ),
            "minItems": 1,
            "uniqueItems": true
        }),
        false,
    )])
}

pub(super) fn target_schema(selector: &AgentTargetSelector, description: &str) -> Value {
    object_schema(vec![(
        "target",
        target_property_schema(selector, Some(description)),
        true,
    )])
}

fn target_property_schema(selector: &AgentTargetSelector, description: Option<&str>) -> Value {
    let mut schema =
        serde_json::Map::from_iter([("type".to_string(), Value::String("string".to_string()))]);
    if let Some(description) = description {
        schema.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    if let AgentTargetSelector::Explicit(agent_ids) = selector {
        schema.insert(
            "enum".to_string(),
            Value::Array(
                agent_ids
                    .iter()
                    .map(|agent_id| Value::String(agent_id.to_string()))
                    .collect(),
            ),
        );
    } else if matches!(selector, AgentTargetSelector::None) {
        schema.insert("enum".to_string(), Value::Array(Vec::new()));
    }
    Value::Object(schema)
}

pub(super) fn object_schema(fields: Vec<(&str, Value, bool)>) -> Value {
    let properties = fields
        .iter()
        .map(|(name, schema, _)| ((*name).to_string(), schema.clone()))
        .collect::<serde_json::Map<_, _>>();
    let required = fields
        .into_iter()
        .filter(|(_, _, required)| *required)
        .map(|(name, _, _)| Value::String(name.to_string()))
        .collect::<Vec<_>>();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

pub(super) fn parse_input<T: for<'de> Deserialize<'de>>(
    tool: &str,
    input: Value,
) -> Result<T, PureError> {
    serde_json::from_value(input)
        .map_err(|error| tool_error(tool, format!("invalid input: {error}")))
}

pub(super) fn parse_agent_id(tool: &str, value: String) -> Result<AgentId, PureError> {
    AgentId::new(value).map_err(|error| tool_error(tool, error.to_string()))
}

pub(super) fn json_output(value: Value) -> Result<ToolOutput, PureError> {
    let description = serde_json::to_string(&value)
        .map_err(|error| tool_error("agent", format!("failed to serialize output: {error}")))?;
    Ok(ToolOutput {
        description,
        truncated: crate::OutputTruncation::empty(),
        output_file: std::path::PathBuf::new(),
        exit_code: None,
        timed_out: false,
        runtime_events: Vec::new(),
    })
}

/// 与 [`json_output`] 相同，但声明更大的模型可见输出硬字节上限。
///
/// 用于 `read_agent_submissions` 等需要完整返回结构化历史的只读查询；仍应配合
/// 分页控制单次返回体积。
pub(super) fn json_output_with_budget(
    value: Value,
    max_bytes: usize,
) -> Result<ToolOutput, PureError> {
    let description = serde_json::to_string(&value)
        .map_err(|error| tool_error("agent", format!("failed to serialize output: {error}")))?;
    Ok(ToolOutput {
        description,
        truncated: crate::OutputTruncation::empty(),
        output_file: std::path::PathBuf::new(),
        exit_code: None,
        timed_out: false,
        runtime_events: vec![crate::tool::ToolRuntimeEvent::OutputBudget { max_bytes }],
    })
}

pub(super) fn tool_error(tool: &str, error: String) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error,
    }
}

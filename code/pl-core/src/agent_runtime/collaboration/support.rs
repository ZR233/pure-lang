use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use pl_protocol::{AgentProfileSnapshot, PureError};
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::{AgentAccessPolicy, AgentSnapshot, AgentTargetSelector, ThreadId};
use super::{ForkTurns, TOOL_SPAWN_AGENT};
use crate::tool::tool_error;
use crate::{AgentRoleId, AgentSession, AgentSessionForkPolicy, ToolResult};

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
    caller: &ThreadId,
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

pub(super) fn agent_path(id: &ThreadId, snapshots: &[AgentSnapshot]) -> Vec<ThreadId> {
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

fn parent_map(snapshots: &[AgentSnapshot]) -> BTreeMap<ThreadId, Option<ThreadId>> {
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

fn root_id(id: &ThreadId, parents: &BTreeMap<ThreadId, Option<ThreadId>>) -> ThreadId {
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

pub(super) fn spawn_schema(policy: &AgentAccessPolicy, profiles: &[AgentProfileSnapshot]) -> Value {
    let profile_ids = profiles
        .iter()
        .filter(|profile| {
            AgentRoleId::new(profile.profile_id.clone())
                .is_ok_and(|role| policy.spawn_roles.contains(&role))
        })
        .map(|profile| Value::String(profile.profile_id.clone()))
        .collect::<Vec<_>>();
    object_schema(vec![
        ("message", json!({ "type": "string" }), true),
        (
            "profileId",
            json!({
                "type": "string",
                "enum": profile_ids,
                "description": "Enabled Agent Profile to freeze for the new child Agent."
            }),
            true,
        ),
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
                "description": "Id of a direct child agent to steer without interruption while refreshing its current turn budget. Only parent-to-direct-child insertion is allowed."
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

pub(super) fn parse_agent_id(tool: &str, value: String) -> Result<ThreadId, PureError> {
    ThreadId::new(value).map_err(|error| tool_error(tool, error.to_string()))
}

pub(super) fn json_output(value: Value) -> Result<ToolResult, PureError> {
    ToolResult::json(value)
        .map_err(|error| tool_error("agent", format!("failed to serialize output: {error}")))
}

/// 与 [`json_output`] 相同，但声明更大的模型可见输出硬字节上限。
///
/// 用于 `read_agent_submissions` 等需要完整返回结构化历史的只读查询；仍应配合
/// 分页控制单次返回体积。
pub(super) fn json_output_with_budget(
    value: Value,
    max_bytes: usize,
) -> Result<ToolResult, PureError> {
    ToolResult::json_with_budget(
        value,
        max_bytes / crate::tool::TOKEN_ESTIMATE_BYTES,
        max_bytes,
    )
    .map_err(|error| tool_error("agent", format!("failed to serialize output: {error}")))
}

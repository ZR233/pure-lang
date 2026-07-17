use std::sync::Arc;
use std::time::Duration;

use pl_protocol::PureError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    AgentAccessPolicy, AgentId, AgentRuntimeHandle, AgentSessionState, AgentSpawnRequest,
    AgentSubmitRequest, AgentTargetSelector, InputDelivery, SessionId,
};
use crate::{AgentRoleId, Tool, ToolContext, ToolEffect, ToolInput, ToolOutput};

const TOOL_SPAWN_AGENT: &str = "spawn_agent";
const TOOL_SEND_INPUT: &str = "send_input";
const TOOL_WAIT_AGENT: &str = "wait_agent";
const TOOL_LIST_AGENTS: &str = "list_agents";
const TOOL_CLOSE_AGENT: &str = "close_agent";
const DEFAULT_WAIT_TIMEOUT_MS: u64 = 30_000;

mod support;
use support::{
    filter_visible, fork_session, json_output, object_schema, parse_agent_id, parse_input,
    send_schema, spawn_schema, target_schema, tool_error, wait_schema,
};

/// 为一次 turn 构造由 `AgentRuntimeHandle` 驱动的协作工具。
///
/// 工具只持有非泛型命令句柄和本轮已编译策略；产品 host 仍负责 lifecycle、
/// repository 与 turn factory，实现不会泄漏进 `ToolContext`。
#[derive(Debug, Clone)]
pub struct AgentCollaborationTools {
    runtime: AgentRuntimeHandle,
    caller: AgentId,
    policy: AgentAccessPolicy,
}

impl AgentCollaborationTools {
    pub fn new(runtime: AgentRuntimeHandle, caller: AgentId, policy: AgentAccessPolicy) -> Self {
        Self {
            runtime,
            caller,
            policy,
        }
    }

    /// 返回可直接注册到 `AgentKernelBuilder` 的五个协作工具。
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        CollaborationToolKind::ALL
            .into_iter()
            .map(|kind| {
                Arc::new(CollaborationTool {
                    kind,
                    runtime: self.runtime.clone(),
                    caller: self.caller.clone(),
                    policy: self.policy.clone(),
                }) as Arc<dyn Tool>
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
enum CollaborationToolKind {
    Spawn,
    Send,
    Wait,
    List,
    Close,
}

impl CollaborationToolKind {
    const ALL: [Self; 5] = [Self::Spawn, Self::Send, Self::Wait, Self::List, Self::Close];

    fn name(self) -> &'static str {
        match self {
            Self::Spawn => TOOL_SPAWN_AGENT,
            Self::Send => TOOL_SEND_INPUT,
            Self::Wait => TOOL_WAIT_AGENT,
            Self::List => TOOL_LIST_AGENTS,
            Self::Close => TOOL_CLOSE_AGENT,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Spawn => "Spawn a child agent using one of the roles allowed for this turn.",
            Self::Send => "Submit input to an accessible agent using an explicit delivery mode.",
            Self::Wait => "Wait until an accessible agent is idle and its input queue is empty.",
            Self::List => "List agents visible to the current collaboration policy.",
            Self::Close => "Close an accessible child agent and its product resources.",
        }
    }
}

#[derive(Debug, Clone)]
struct CollaborationTool {
    kind: CollaborationToolKind,
    runtime: AgentRuntimeHandle,
    caller: AgentId,
    policy: AgentAccessPolicy,
}

impl Tool for CollaborationTool {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn input_schema(&self) -> Value {
        match self.kind {
            CollaborationToolKind::Spawn => spawn_schema(&self.policy),
            CollaborationToolKind::Send => send_schema(),
            CollaborationToolKind::Wait => wait_schema(),
            CollaborationToolKind::List => object_schema(Vec::new()),
            CollaborationToolKind::Close => target_schema("Agent id to close."),
        }
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        matches!(
            self.kind,
            CollaborationToolKind::Send | CollaborationToolKind::Wait | CollaborationToolKind::List
        )
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::AgentControl)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutput, PureError>> + Send + 'a>,
    > {
        Box::pin(async move {
            match self.kind {
                CollaborationToolKind::Spawn => self.spawn(input, context).await,
                CollaborationToolKind::Send => self.send(input).await,
                CollaborationToolKind::Wait => self.wait(input).await,
                CollaborationToolKind::List => self.list(input).await,
                CollaborationToolKind::Close => self.close(input).await,
            }
        })
    }
}

impl CollaborationTool {
    async fn spawn(&self, input: ToolInput, context: ToolContext) -> Result<ToolOutput, PureError> {
        let args: SpawnArgs = parse_input(TOOL_SPAWN_AGENT, input.arguments)?;
        let role = AgentRoleId::new(args.role)
            .map_err(|error| tool_error(TOOL_SPAWN_AGENT, error.to_string()))?;
        if !self.policy.spawn_roles.contains(&role) {
            return Err(tool_error(
                TOOL_SPAWN_AGENT,
                format!("role `{role}` is not allowed for this turn"),
            ));
        }
        let session_id = SessionId::generate();
        let session = AgentSessionState {
            id: session_id.clone(),
            metadata: serde_json::Value::Null,
            session: fork_session(&context.parent_session, args.fork_turns)?,
            usage: pl_model::TokenUsage::default(),
            last_context_tokens: None,
            trace_sequence: 0,
        };
        let mut metadata = match args.metadata {
            Value::Object(metadata) => metadata,
            Value::Null => serde_json::Map::new(),
            _ => {
                return Err(tool_error(
                    TOOL_SPAWN_AGENT,
                    "metadata must be an object".to_string(),
                ));
            }
        };
        metadata.insert(
            "requestingToolCallId".to_string(),
            Value::String(input.tool_id),
        );
        metadata.insert(
            "workspaceRoot".to_string(),
            Value::String(context.workspace_root.to_string_lossy().to_string()),
        );
        let result = self
            .runtime
            .spawn(AgentSpawnRequest {
                parent_id: self.caller.clone(),
                role,
                session,
                initial_message: Some(args.message),
                metadata: Value::Object(metadata),
            })
            .await
            .map_err(|error| tool_error(TOOL_SPAWN_AGENT, error.to_string()))?;
        json_output(json!({
            "agentId": result.snapshot.identity.id,
            "sessionId": session_id,
            "turnId": result.initial_turn_id,
            "snapshot": result.snapshot,
        }))
    }

    async fn send(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: SendArgs = parse_input(TOOL_SEND_INPUT, input.arguments)?;
        let target = parse_agent_id(TOOL_SEND_INPUT, args.target)?;
        self.authorize(&self.policy.message_targets, &target)
            .await?;
        let session_id = args
            .session_id
            .map(SessionId::new)
            .transpose()
            .map_err(|error| tool_error(TOOL_SEND_INPUT, error.to_string()))?
            .unwrap_or(SessionId::new(input.session_id).map_err(|error| {
                tool_error(
                    TOOL_SEND_INPUT,
                    format!("invalid current session id: {error}"),
                )
            })?);
        let turn_id = self
            .runtime
            .submit(
                target.clone(),
                AgentSubmitRequest {
                    session_id,
                    message: args.message,
                    metadata: args.metadata,
                    delivery: args.delivery,
                },
            )
            .await
            .map_err(|error| tool_error(TOOL_SEND_INPUT, error.to_string()))?;
        json_output(json!({ "target": target, "turnId": turn_id }))
    }

    async fn wait(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: WaitArgs = parse_input(TOOL_WAIT_AGENT, input.arguments)?;
        let target = parse_agent_id(TOOL_WAIT_AGENT, args.target)?;
        self.authorize(&self.policy.wait_targets, &target).await?;
        let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS));
        match self.runtime.wait_timeout(target.clone(), timeout).await {
            Ok(result) => json_output(json!({
                "target": target,
                "timedOut": false,
                "snapshot": result.snapshot,
                "lastTurn": result.last_turn,
            })),
            Err(super::AgentRuntimeError::TimedOut) => {
                json_output(json!({ "target": target, "timedOut": true }))
            }
            Err(error) => Err(tool_error(TOOL_WAIT_AGENT, error.to_string())),
        }
    }

    async fn list(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let _: EmptyArgs = parse_input(TOOL_LIST_AGENTS, input.arguments)?;
        let snapshots = self
            .runtime
            .list()
            .await
            .map_err(|error| tool_error(TOOL_LIST_AGENTS, error.to_string()))?;
        let visible = filter_visible(&snapshots, &self.caller, &self.policy.list_targets);
        json_output(json!({ "agents": visible }))
    }

    async fn close(&self, input: ToolInput) -> Result<ToolOutput, PureError> {
        let args: TargetArgs = parse_input(TOOL_CLOSE_AGENT, input.arguments)?;
        let target = parse_agent_id(TOOL_CLOSE_AGENT, args.target)?;
        if target == self.caller {
            return Err(tool_error(
                TOOL_CLOSE_AGENT,
                "an agent cannot close itself".to_string(),
            ));
        }
        self.authorize(&self.policy.close_targets, &target).await?;
        let snapshot = self
            .runtime
            .close(target)
            .await
            .map_err(|error| tool_error(TOOL_CLOSE_AGENT, error.to_string()))?;
        json_output(json!({ "snapshot": snapshot }))
    }

    async fn authorize(
        &self,
        selector: &AgentTargetSelector,
        target: &AgentId,
    ) -> Result<(), PureError> {
        let snapshots = self
            .runtime
            .list()
            .await
            .map_err(|error| tool_error(self.kind.name(), error.to_string()))?;
        let allowed = filter_visible(&snapshots, &self.caller, selector)
            .iter()
            .any(|snapshot| &snapshot.identity.id == target);
        if allowed {
            Ok(())
        } else {
            Err(tool_error(
                self.kind.name(),
                format!("agent `{target}` is not accessible for this turn"),
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpawnArgs {
    message: String,
    role: String,
    #[serde(default)]
    fork_turns: ForkTurns,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SendArgs {
    target: String,
    message: String,
    session_id: Option<String>,
    #[serde(default)]
    delivery: InputDelivery,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WaitArgs {
    target: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetArgs {
    target: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ForkTurns {
    #[default]
    None,
    All,
    Last(usize),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{AgentActivityState, AgentIdentity, AgentLifecycleState, AgentSnapshot};

    #[test]
    fn spawn_schema_uses_policy_roles() {
        let policy = AgentAccessPolicy {
            spawn_roles: BTreeSet::from([
                AgentRoleId::new("researcher").unwrap(),
                AgentRoleId::new("writer").unwrap(),
            ]),
            ..AgentAccessPolicy::default()
        };
        let schema = spawn_schema(&policy);
        assert_eq!(
            schema["properties"]["role"]["enum"],
            json!(["researcher", "writer"])
        );
    }

    #[test]
    fn tree_selector_keeps_one_root_component() {
        let root = snapshot("root", None);
        let child = snapshot("child", Some("root"));
        let sibling_root = snapshot("other", None);
        let visible = filter_visible(
            &[root, child, sibling_root],
            &AgentId::new("child").unwrap(),
            &AgentTargetSelector::Tree,
        );
        assert_eq!(
            visible
                .into_iter()
                .map(|snapshot| snapshot.identity.id.to_string())
                .collect::<Vec<_>>(),
            vec!["root", "child"]
        );
    }

    fn snapshot(id: &str, parent: Option<&str>) -> AgentSnapshot {
        AgentSnapshot {
            identity: AgentIdentity {
                id: AgentId::new(id).unwrap(),
                parent_id: parent.map(|id| AgentId::new(id).unwrap()),
                role: AgentRoleId::new("test").unwrap(),
                depth: parent.is_some() as u32,
            },
            lifecycle: AgentLifecycleState::Active,
            activity: AgentActivityState::Idle,
            active_turn_id: None,
            active_session_id: None,
            pending_inputs: 0,
            last_turn: None,
            revision: 1,
            event_sequence: 1,
            updated_at: 0,
        }
    }
}

//! Agent control tool frontends; directory ownership and access decisions belong to the product host.
use pl_core::{
    context::OpaquePayload,
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use std::sync::Arc;

/// A message whose id is assigned from the trusted core call identity, never supplied by model arguments.
#[derive(Debug)]
pub struct AgentMessage {
    pub id: String,
    pub target: String,
    pub message: String,
}

/// Child creation parameters; Profile interpretation and permissions belong to Studio.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSpawn {
    pub profile_id: String,
    pub message: String,
    /// Inherits conversation records only; the child receives its own Profile instructions.
    #[serde(default)]
    pub fork_turns: AgentHistory,
    /// Optional project-relative write directories, accepted only by directory Profiles.
    pub writable_paths: Option<Vec<String>>,
    /// Product-owned creation metadata; never interpreted as framework permissions.
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Product tool syntax for selecting a core conversation window.
#[derive(Debug, Clone, Copy, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AgentHistory {
    #[default]
    None,
    All,
    Last(std::num::NonZeroUsize),
}

impl AgentHistory {
    /// Converts the explicit selection without copying a model session or tool state.
    pub fn inheritance(self) -> pl_core::context::HistoryInheritance {
        match self {
            Self::None => pl_core::context::HistoryInheritance::Empty,
            Self::All => pl_core::context::HistoryInheritance::All,
            Self::Last(count) => pl_core::context::HistoryInheritance::LastUserTurns(count),
        }
    }
}

/// Explicit disposition for a product-owned child workspace after its Thread closes.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum AgentWorkspaceDisposition {
    /// Retain the workspace and its recoverable lease for later inspection or integration.
    #[default]
    Preserve,
    /// Request checked cleanup of the child-owned workspace after integration.
    Cleanup,
}

/// Product coordinator ports. Return the original business facts and the chosen model projection together.
pub trait AgentControlHost: Send + Sync + std::fmt::Debug + 'static {
    fn spawn(
        &self,
        context: CallContext,
        request: AgentSpawn,
    ) -> impl std::future::Future<Output = Result<ToolOutput, ToolError>> + Send;
    fn send(
        &self,
        caller: &str,
        message: AgentMessage,
    ) -> impl std::future::Future<Output = Result<ToolOutput, ToolError>> + Send;
    fn list(
        &self,
        caller: &str,
    ) -> impl std::future::Future<Output = Result<ToolOutput, ToolError>> + Send;
    fn interrupt(
        &self,
        caller: &str,
        target: &str,
    ) -> impl std::future::Future<Output = Result<ToolOutput, ToolError>> + Send;
    fn close(
        &self,
        caller: &str,
        target: &str,
        disposition: AgentWorkspaceDisposition,
    ) -> impl std::future::Future<Output = Result<ToolOutput, ToolError>> + Send;
}

/// Stable agent-control operations; none grants permission to bypass the host's relationship checks.
#[derive(Debug, Clone, Copy)]
pub enum AgentControlKind {
    Spawn,
    Send,
    List,
    Interrupt,
    Close,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Target {
    target: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CloseInput {
    target: String,
    #[serde(default)]
    workspace_disposition: AgentWorkspaceDisposition,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SendInput {
    target: String,
    message: String,
}

impl AgentControlKind {
    /// Stable declaration without current agents, statuses or execution identities.
    pub fn declaration(self) -> pl_protocol::ToolSpec {
        match self {
            Self::Spawn => pl_protocol::ToolSpec::function(
                "spawn_agent",
                "Create a child using an authorized profile and optional recent conversation history. The host validates permissions and limits.",
                schemars::schema_for!(AgentSpawn).to_value(),
            ),
            Self::Send => pl_protocol::ToolSpec::function(
                "send_message",
                "Send a runtime message to a direct child and wake it if that message remains unconsumed.",
                schemars::schema_for!(SendInput).to_value(),
            ),
            Self::List => pl_protocol::ToolSpec::function(
                "list_agents",
                "List agents visible within the caller's thread tree.",
                schemars::schema_for!(Empty).to_value(),
            ),
            Self::Interrupt => pl_protocol::ToolSpec::function(
                "interrupt_agent",
                "Pause queued execution and interrupt a descendant agent's active turn.",
                schemars::schema_for!(Target).to_value(),
            ),
            Self::Close => pl_protocol::ToolSpec::function(
                "close_agent",
                "Close a descendant agent and its descendants. Preserve workspaces by default; request cleanup only after integration. Failed resource closures remain retryable.",
                schemars::schema_for!(CloseInput).to_value(),
            ),
        }
    }
}

/// One Thread's frontend over a shared product coordination service.
#[derive(Debug)]
pub struct ThreadAgentControl<H> {
    host: Arc<H>,
    kind: AgentControlKind,
}

impl<H: AgentControlHost> ThreadAgentControl<H> {
    /// Associates an operation with its product-owned coordinator.
    pub fn new(host: Arc<H>, kind: AgentControlKind) -> Self {
        Self { host, kind }
    }

    /// Transfers an executor; product coordination remains behind the host port.
    ///
    /// # Errors
    /// Returns invalid registry identity errors.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let name = match self.kind {
            AgentControlKind::Spawn => "spawn_agent",
            AgentControlKind::Send => "send_message",
            AgentControlKind::List => "list_agents",
            AgentControlKind::Interrupt => "interrupt_agent",
            AgentControlKind::Close => "close_agent",
        };
        let kind = self.kind;
        let registration = Registration::new(name.into(), declaration, self)?;
        Ok(match kind {
            AgentControlKind::List => registration,
            AgentControlKind::Spawn
            | AgentControlKind::Send
            | AgentControlKind::Interrupt
            | AgentControlKind::Close => registration.foreground(),
        })
    }
}

impl<H: AgentControlHost> Tool for ThreadAgentControl<H> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(std::io::Error::other(
                "unsupported agent control encoding",
            )));
        }
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        match self.kind {
            AgentControlKind::Spawn => {
                let request: AgentSpawn =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                self.host.spawn(context, request).await
            }
            AgentControlKind::Send => {
                let input: SendInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                self.host
                    .send(
                        &context.thread_id,
                        AgentMessage {
                            id: format!(
                                "agent-message:{}:{}:{}",
                                context.thread_id.len(),
                                context.thread_id,
                                context.call_id
                            ),
                            target: input.target,
                            message: input.message,
                        },
                    )
                    .await
            }
            AgentControlKind::List => {
                let _: Empty = serde_json::from_str(input.content()).map_err(ToolError::new)?;
                self.host.list(&context.thread_id).await
            }
            AgentControlKind::Interrupt => {
                let target: Target =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                self.host
                    .interrupt(&context.thread_id, &target.target)
                    .await
            }
            AgentControlKind::Close => {
                let target: CloseInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                self.host
                    .close(
                        &context.thread_id,
                        &target.target,
                        target.workspace_disposition,
                    )
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[derive(Debug)]
    struct QueryHost;

    impl AgentControlHost for QueryHost {
        async fn spawn(&self, _: CallContext, _: AgentSpawn) -> Result<ToolOutput, ToolError> {
            panic!("control batch must be rejected before execution")
        }
        async fn send(&self, _: &str, _: AgentMessage) -> Result<ToolOutput, ToolError> {
            panic!("control batch must be rejected before execution")
        }
        async fn list(&self, caller: &str) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::new(OpaquePayload::text(caller), Vec::new()))
        }
        async fn interrupt(&self, _: &str, _: &str) -> Result<ToolOutput, ToolError> {
            panic!("control batch must be rejected before execution")
        }
        async fn close(
            &self,
            _: &str,
            _: &str,
            _: AgentWorkspaceDisposition,
        ) -> Result<ToolOutput, ToolError> {
            panic!("control batch must be rejected before execution")
        }
    }

    struct QueryBatch(&'static str);
    impl pl_core::model::ModelSession for QueryBatch {
        async fn prepare(
            &mut self,
            request: pl_core::model::ModelRequest,
        ) -> Result<pl_core::model::PreparedModelCall, pl_core::model::ModelError> {
            use pl_core::model::*;
            let query = self.0;
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: Vec::new(),
                    private_context: None,
                    usage: Default::default(),
                    tool_calls: ["read_file", query]
                        .into_iter()
                        .map(|name| ModelToolCall {
                            call_id: name.into(),
                            tool_id: name.into(),
                            arguments: crate::test_support::input(serde_json::json!({})),
                        })
                        .collect(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), pl_core::model::ModelError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct Read;
    impl Tool for Read {
        async fn execute(&self, _: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::new(
                OpaquePayload::text("file contents"),
                Vec::new(),
            ))
        }
    }

    #[tokio::test]
    async fn list_agents_shares_read_batches_while_coordination_mutations_remain_solo() {
        use pl_core::{model::DynModelSession, thread::*};
        for (kind, name) in [
            (AgentControlKind::List, "list_agents"),
            (AgentControlKind::Spawn, "spawn_agent"),
            (AgentControlKind::Send, "send_message"),
            (AgentControlKind::Interrupt, "interrupt_agent"),
            (AgentControlKind::Close, "close_agent"),
        ] {
            let thread =
                ThreadHandle::start("caller".into(), DynModelSession::new(QueryBatch(name)))
                    .unwrap();
            thread
                .register_tools(vec![
                    Registration::new("read_file".into(), OpaquePayload::text("read"), Read)
                        .unwrap(),
                    ThreadAgentControl::new(Arc::new(QueryHost), kind)
                        .registration(OpaquePayload::text(name))
                        .unwrap(),
                ])
                .await
                .unwrap();
            let result = thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: Vec::new(),
                    cancellation: Default::default(),
                })
                .await;
            if matches!(kind, AgentControlKind::List) {
                result.expect("read-only collaboration query must not invalidate a read batch");
                let (read, list) = tokio::join!(
                    thread.execute_tool("read_file".into(), Default::default()),
                    thread.execute_tool("list_agents".into(), Default::default()),
                );
                read.unwrap();
                list.unwrap();
                assert_eq!(thread.snapshot().tasks.len(), 2);
                assert!(
                    thread
                        .snapshot()
                        .tasks
                        .values()
                        .all(|task| task.status == task::TaskStatus::Succeeded)
                );
            } else {
                assert!(
                    matches!(result, Err(ThreadError::InvalidOutput)),
                    "{name}: {result:?}"
                );
                assert!(thread.snapshot().tasks.is_empty());
            }
            thread.close().await.unwrap();
        }
    }

    #[test]
    fn close_defaults_to_preserve_and_requires_typed_explicit_cleanup() {
        let preserved: CloseInput = serde_json::from_str(r#"{"target":"child"}"#).unwrap();
        assert_eq!(
            preserved.workspace_disposition,
            AgentWorkspaceDisposition::Preserve
        );
        let cleaned: CloseInput =
            serde_json::from_str(r#"{"target":"child","workspaceDisposition":"cleanup"}"#).unwrap();
        assert_eq!(
            cleaned.workspace_disposition,
            AgentWorkspaceDisposition::Cleanup
        );
        assert!(
            serde_json::from_str::<CloseInput>(
                r#"{"target":"child","workspace_disposition":"cleanup"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<CloseInput>(
                r#"{"target":"child","workspaceDisposition":"forceDelete"}"#
            )
            .is_err()
        );
    }
}

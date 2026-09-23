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
    /// Brief task description shown in the child list; required, nonempty, at most 80 Unicode characters.
    pub task_summary: AgentTaskSummary,
    /// Full self-contained task: goals, approved contract, facts, interfaces, steps, ownership and verification. No application length limit; include pseudocode when useful.
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

/// Validated single-line task title, supplied by the caller before child resources are allocated.
#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(try_from = "String")]
#[schemars(with = "String", extend("minLength" = 1, "maxLength" = 80))]
pub struct AgentTaskSummary(String);

#[derive(Debug, thiserror::Error)]
pub enum AgentTaskSummaryError {
    #[error("taskSummary cannot be empty")]
    Empty,
    #[error("taskSummary exceeds 80 Unicode characters")]
    TooLong,
}

impl TryFrom<String> for AgentTaskSummary {
    type Error = AgentTaskSummaryError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            return Err(AgentTaskSummaryError::Empty);
        }
        if normalized.chars().count() > 80 {
            return Err(AgentTaskSummaryError::TooLong);
        }
        Ok(Self(normalized))
    }
}

impl AgentTaskSummary {
    pub fn as_str(&self) -> &str {
        &self.0
    }
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
    /// Full continuation, including new decisions, superseded requirements and acceptance criteria. No application length limit.
    message: String,
}

impl AgentControlKind {
    /// Stable declaration without current agents, statuses or execution identities.
    pub fn declaration(self) -> pl_protocol::ToolSpec {
        match self {
            Self::Spawn => pl_protocol::ToolSpec::function(
                "spawn_agent",
                "Create a child using an authorized profile and optional recent conversation history. Provide taskSummary: a brief task description (1–80 Unicode characters) shown in the child list, and message: the complete instructions. The host validates permissions and limits.",
                schemars::schema_for!(AgentSpawn).to_value(),
            ),
            Self::Send => pl_protocol::ToolSpec::function(
                "send_message",
                "Send a prompt to a direct child. Interrupt its active turn and continue with this message after tool cleanup; start it if idle. The receipt confirms acceptance, not completion.",
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

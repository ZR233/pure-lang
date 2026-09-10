//! Public Thread state, input contracts and typed outcomes.
use super::*;

/// Immutable facts published after each accepted request or model-output commit.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    #[serde(skip)]
    pub model_progress: Option<crate::model::ActiveModelProgress>,
    /// Ephemeral producer previews for running tasks; never persisted or used as model context.
    #[serde(skip)]
    pub tool_progress: std::collections::BTreeMap<String, Vec<ContextContent>>,
    #[serde(default)]
    pub permissions: std::collections::BTreeMap<String, permissions::PermissionRecord>,
    #[serde(default)]
    pub permission_changes: Arc<[permissions::PermissionRecord]>,
    #[serde(default)]
    pub wake_messages_through: u64,
    #[serde(skip)]
    pub input_execution: input::InputExecution,
    #[serde(default)]
    pub inputs: Arc<[input::InputRecord]>,
    #[serde(default)]
    pub input_changes: Arc<[input::InputChange]>,
    /// Live session availability; replay never constructs physical model resources.
    #[serde(skip)]
    pub model_available: bool,
    /// Completed executions retained in memory until their atomic result commit succeeds.
    #[serde(skip)]
    pub pending_tool_commits: Vec<String>,
    #[serde(default)]
    pub tasks: std::collections::BTreeMap<String, task::TaskRecord>,
    #[serde(default)]
    pub task_changes: Arc<[task::TaskRecord]>,
    pub commit_sequence: u64,
    pub persistence: cold::PersistenceState,
    pub lifecycle: ThreadLifecycle,
    pub context: ContextSnapshot,
    pub attempts: Arc<[RequestAttempt]>,
    pub discovered_tools: Arc<[ModelToolDeclaration]>,
    pub turns: Arc<[TurnRecord]>,
    pub private_context: Option<OpaquePayload>,
    pub deliveries: Arc<[ToolDelivery]>,
    pub context_replacements: Arc<[ContextReplacement]>,
    pub runtime_facts: Arc<[RuntimeFact]>,
    pub extensions: std::collections::BTreeMap<String, extensions::ExtensionRecord>,
    pub extension_sequence: u64,
    pub inbox: Arc<[inbox::InboxRecord]>,
    pub consumed_messages: u64,
    pub interactions: std::collections::BTreeMap<String, interactions::InteractionRecord>,
    pub interaction_changes: Arc<[interactions::InteractionRecord]>,
    pub extension_changes: Arc<[extensions::ExtensionChange]>,
}

/// Complete current facts from a stable host source. Empty content explicitly invalidates it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFact {
    pub source_id: String,
    pub content: Vec<ContextContent>,
}

/// Explicit host-selected context transformation, independent of summarizer implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ContextReplacementReason {
    Compaction,
    Rewind,
    Rebuild,
}

/// An immutable context replacement fact; prior content remains available for history replay.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextReplacement {
    pub reason: ContextReplacementReason,
    pub previous: ContextSnapshot,
    pub current: ContextSnapshot,
    pub previous_private_context: Option<OpaquePayload>,
}

/// Candidate context supplied by a trusted host, with compare-and-swap admission.
#[derive(Debug)]
pub struct ReplaceContext {
    pub expected_revision: u64,
    pub reason: ContextReplacementReason,
    pub records: Vec<ContextRecord>,
}

/// Owner lifecycle, independent of product modes and workflows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ThreadLifecycle {
    #[default]
    Open,
    Closing,
    Closed,
}

/// An admitted model request and its terminal outcome, retained even after failure.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestAttempt {
    #[serde(default)]
    pub request_metadata: Option<OpaquePayload>,
    #[serde(default)]
    pub tool_projection: Option<OpaquePayload>,
    pub turn_id: String,
    pub attempt_id: String,
    pub retry_of: Option<String>,
    pub input: ContextSnapshot,
    pub tools: Arc<[ModelToolDeclaration]>,
    pub outcome: AttemptOutcome,
    pub input_estimate: Option<crate::model::TokenEstimate>,
}

/// A provider result only becomes canonical through a Thread commit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum AttemptOutcome {
    Running,
    Interrupted,
    Committed(ModelStepOutput),
    Cancelled {
        result: Result<ModelStepOutput, Arc<ModelError>>,
    },
    Failed(Arc<ModelError>),
    Rejected(ModelStepOutput),
}

impl RequestAttempt {
    /// Returns usage observed for this attempt, including cancelled and rejected responses.
    pub fn usage(&self) -> Option<&crate::model::ModelUsage> {
        match &self.outcome {
            AttemptOutcome::Running | AttemptOutcome::Interrupted => None,
            AttemptOutcome::Committed(output) | AttemptOutcome::Rejected(output) => {
                Some(&output.usage)
            }
            AttemptOutcome::Failed(error) => Some(&error.usage),
            AttemptOutcome::Cancelled { result } => Some(match result {
                Ok(output) => &output.usage,
                Err(error) => &error.usage,
            }),
        }
    }
}

/// Whether a call completed inside the delivery window or remains owned by the Thread.
#[derive(Debug, Clone)]
pub enum ToolDispatch {
    Completed(crate::tool::ToolOutput),
    Running(task::TaskRecord),
}

/// Actual destination of a tool's frozen model projection.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ToolDeliveryTarget {
    #[default]
    CallResult,
    Inbox {
        message_id: String,
    },
}

/// The complete tool result committed with its original call identity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDelivery {
    #[serde(default)]
    pub target: ToolDeliveryTarget,
    pub call_id: String,
    pub tool_id: String,
    pub output: crate::tool::ToolOutput,
    pub delivered_context: Vec<ContextContent>,
    pub outcome: ToolOutcome,
}

/// Execution facts are typed and cannot be forged through result payload content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ToolOutcome {
    Succeeded,
    Cancelled,
    Interrupted,
    Failed(Arc<crate::tool::opaque::ToolError>),
}

/// One step's supplied input. Retry uses a new attempt identity and may omit new content.
#[derive(Debug)]
pub struct StepInput {
    pub turn_id: String,
    pub attempt_id: String,
    pub content: Vec<ContextContent>,
    pub cancellation: CancellationToken,
}

/// A bounded Turn executed by the same owner that holds model and tool instances.
#[derive(Debug)]
pub struct TurnInput {
    pub turn_id: String,
    pub attempt_prefix: String,
    pub content: Vec<ContextContent>,
    pub max_model_steps: std::num::NonZeroU32,
    pub cancellation: CancellationToken,
}

/// Terminal reason for a successfully observed Turn execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnOutcome {
    Completed,
    ToolCompleted,
    WaitingInteraction,
    StepLimit,
}

/// Durable Turn execution state, independent of provider response status.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum TurnState {
    Running,
    Finished(TurnOutcome),
    Cancelled,
    Interrupted,
    Failed { description: String },
}

/// One accepted bounded Turn and its final disposition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
    /// Measured execution time; interrupted recovery may have no final duration.
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
    /// Input whose execution opened this Turn, including attempts that fail before model admission.
    #[serde(default)]
    pub input_id: Option<String>,
    pub turn_id: String,
    pub state: TurnState,
    pub model_steps: u32,
}

/// Observed final model step plus the reason no further inference was started.
#[derive(Debug, Clone)]
pub struct TurnCompletion {
    pub model_steps: u32,
    pub outcome: TurnOutcome,
    pub last_output: ModelStepOutput,
}

/// Admission policy selected by the host; it never interprets provider-specific content.
#[derive(Debug, Clone, Copy, Default)]
pub enum ContextCapacity {
    #[default]
    Unbounded,
    RequireExact {
        max_input_tokens: u64,
    },
    AcceptApproximate {
        max_input_tokens: u64,
    },
    AllowUnknown {
        max_input_tokens: u64,
    },
}

impl ContextCapacity {
    pub(super) fn admit(
        self,
        estimate: Option<crate::model::TokenEstimate>,
    ) -> Result<(), ThreadError> {
        use crate::model::EstimateAccuracy;
        let (limit, accepts_approximate, accepts_unknown) = match self {
            Self::Unbounded => return Ok(()),
            Self::RequireExact { max_input_tokens } => (max_input_tokens, false, false),
            Self::AcceptApproximate { max_input_tokens } => (max_input_tokens, true, false),
            Self::AllowUnknown { max_input_tokens } => (max_input_tokens, true, true),
        };
        match estimate {
            None if !accepts_unknown => Err(ThreadError::UnknownCapacity),
            Some(estimate)
                if estimate.accuracy == EstimateAccuracy::Approximate && !accepts_approximate =>
            {
                Err(ThreadError::UnknownCapacity)
            }
            Some(estimate) if estimate.tokens > limit => {
                Err(ThreadError::ContextCapacity { estimate, limit })
            }
            Some(_) | None => Ok(()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ThreadError {
    #[error("tool registration was rejected and cleanup remains pending: {0}")]
    RejectedTools(#[source] Box<crate::tool::opaque::RejectedTools>),
    #[error("input requires an idle Thread with no pending input")]
    InputRequiresIdle,
    #[error("steering input requires a running Turn")]
    InputRequiresActiveTurn,
    #[error("input is being prepared for model admission")]
    InputInUse,
    #[error("input was already consumed")]
    InputConsumed,
    #[error("Thread model session is unavailable; replace the model binding to continue")]
    ModelUnavailable,
    #[error("frozen tool permissions were revoked before admission or execution")]
    ToolPermissionRevoked,
    #[error("completed tool results are waiting for a successful Thread commit")]
    PendingToolCommit,
    #[error("task control is not granted to this tool")]
    TaskAccessDenied,
    #[error("task control caller is no longer running")]
    TaskAccessExpired,
    #[error("a task cannot wait for itself")]
    TaskSelfWait,
    #[error("Thread owner is closed")]
    Closed,
    #[error("request cancelled")]
    Cancelled,
    #[error("request identity is empty or was already admitted")]
    InvalidIdentity,
    #[error("extension {id} revision conflict: expected {expected:?}, actual {actual:?}")]
    ExtensionConflict {
        id: String,
        expected: Option<u64>,
        actual: Option<u64>,
    },
    #[error("extension sequence conflict: expected {expected}, actual {actual}")]
    ExtensionSequenceConflict { expected: u64, actual: u64 },
    #[error("model output does not match the admitted request")]
    InvalidOutput,
    #[error("context revision exhausted")]
    RevisionExhausted,
    #[error("context revision conflict: expected {expected}, actual {actual}")]
    ContextConflict { expected: u64, actual: u64 },
    #[error("context contains an empty or duplicate record identity")]
    InvalidContext,
    #[error("Thread persistence is blocked: {0}")]
    Storage(#[source] Arc<cold::ColdStoreError>),
    #[error("new model execution is paused by cold-storage pressure")]
    StoragePressure,
    #[error("model did not provide an estimate with the required accuracy")]
    UnknownCapacity,
    #[error("model input estimate {estimate:?} exceeds capacity {limit}")]
    ContextCapacity {
        estimate: crate::model::TokenEstimate,
        limit: u64,
    },
    #[error("invalid context relationships: {0}")]
    Context(#[from] crate::context::ContextError),
    #[error("tool call is missing or already delivered")]
    MissingCall,
    #[error("pending tool calls must be delivered before another model step")]
    PendingTools,
    #[error("a host interaction must be resolved before model execution can continue")]
    PendingInteraction,
    #[error("tool execution failed: {0}")]
    Tool(#[source] Arc<crate::tool::opaque::ToolError>),
    #[error("invalid tool registry: {0}")]
    Registry(#[from] crate::tool::opaque::RegistryError),
    #[error("model operation failed: {0}")]
    Model(#[source] Arc<ModelError>),
}

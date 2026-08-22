use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceToolPart {
    invocation: TraceToolInvocation,
    state: TraceToolState,
}

impl TraceToolPart {
    pub fn started(invocation: TraceToolInvocation) -> Self {
        Self {
            invocation,
            state: TraceToolState::Started(StartedTraceTool),
        }
    }

    pub fn streaming(invocation: TraceToolInvocation) -> Self {
        Self {
            invocation,
            state: TraceToolState::Streaming(StreamingTraceTool),
        }
    }

    pub fn invocation(&self) -> &TraceToolInvocation {
        &self.invocation
    }

    pub fn state(&self) -> &TraceToolState {
        &self.state
    }

    pub fn terminal_output(&self) -> Option<&TraceToolOutput> {
        match &self.state {
            TraceToolState::Succeeded(state) => Some(&state.output),
            TraceToolState::Failed(state) => state.output.as_ref(),
            TraceToolState::Started(_)
            | TraceToolState::Streaming(_)
            | TraceToolState::AwaitingApproval(_)
            | TraceToolState::Approved(_)
            | TraceToolState::Running(_)
            | TraceToolState::Denied(_)
            | TraceToolState::Cancelled(_) => None,
        }
    }

    pub(super) fn update_invocation(
        &self,
        invocation: TraceToolInvocation,
    ) -> Result<Self, &'static str> {
        if self.state.is_terminal() {
            return Err("terminal tool invocation is immutable");
        }
        let mut next = self.clone();
        next.invocation = invocation;
        Ok(next)
    }

    pub(super) fn append_arguments(&self, delta: &str) -> Result<Self, &'static str> {
        if !matches!(
            self.state,
            TraceToolState::Started(_) | TraceToolState::Streaming(_)
        ) {
            return Err("tool argument delta requires started or streaming state");
        }
        let mut next = self.clone();
        next.invocation.arguments.push_str(delta);
        next.state = TraceToolState::Streaming(StreamingTraceTool);
        Ok(next)
    }

    pub(super) fn append_result(&self, _delta: &str) -> Result<Self, &'static str> {
        Err("tool result deltas require a streaming provider result owner")
    }

    pub(super) fn enter(&self, phase: TraceToolActivePhase) -> Result<Self, &'static str> {
        let allowed = matches!(
            (&self.state, phase),
            (
                TraceToolState::Started(_) | TraceToolState::Streaming(_),
                TraceToolActivePhase::AwaitingApproval,
            ) | (
                TraceToolState::AwaitingApproval(_),
                TraceToolActivePhase::Approved
            ) | (
                TraceToolState::Started(_)
                    | TraceToolState::Streaming(_)
                    | TraceToolState::Approved(_),
                TraceToolActivePhase::Running,
            )
        );
        if !allowed {
            return Err("illegal tool lifecycle transition");
        }
        let mut next = self.clone();
        next.state = match phase {
            TraceToolActivePhase::AwaitingApproval => {
                TraceToolState::AwaitingApproval(AwaitingApprovalTraceTool)
            }
            TraceToolActivePhase::Approved => TraceToolState::Approved(ApprovedTraceTool),
            TraceToolActivePhase::Running => TraceToolState::Running(RunningTraceTool),
        };
        Ok(next)
    }

    pub(super) fn succeed(&self, output: TraceToolOutput) -> Result<Self, &'static str> {
        if !matches!(
            self.state,
            TraceToolState::Started(_)
                | TraceToolState::Streaming(_)
                | TraceToolState::Approved(_)
                | TraceToolState::Running(_)
        ) {
            return Err("tool success requires an executable active state");
        }
        let mut next = self.clone();
        next.state = TraceToolState::Succeeded(SucceededTraceTool { output });
        Ok(next)
    }

    pub(super) fn fail(
        &self,
        failure: TraceToolFailure,
        output: Option<TraceToolOutput>,
    ) -> Result<Self, &'static str> {
        if self.state.is_terminal() {
            return Err("terminal tool state cannot fail again");
        }
        let mut next = self.clone();
        next.state = TraceToolState::Failed(FailedTraceTool { failure, output });
        Ok(next)
    }

    pub(super) fn deny(&self, reason: String) -> Result<Self, &'static str> {
        if !matches!(
            self.state,
            TraceToolState::Started(_)
                | TraceToolState::Streaming(_)
                | TraceToolState::AwaitingApproval(_)
        ) {
            return Err("tool denial requires a pre-execution state");
        }
        let mut next = self.clone();
        next.state = TraceToolState::Denied(DeniedTraceTool { reason });
        Ok(next)
    }

    pub(super) fn cancel(&self, cause: TraceToolCancellationCause) -> Result<Self, &'static str> {
        if self.state.is_terminal() {
            return Err("terminal tool state cannot be cancelled");
        }
        let mut next = self.clone();
        next.state = TraceToolState::Cancelled(CancelledTraceTool { cause });
        Ok(next)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceToolInvocation {
    tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_item_id: Option<String>,
    name: String,
    arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    working_directory: Option<String>,
}

impl TraceToolInvocation {
    pub fn new(tool_call_id: String, name: String, arguments: String) -> Self {
        Self {
            tool_call_id,
            call_id: None,
            provider_item_id: None,
            name,
            arguments,
            working_directory: None,
        }
    }

    pub fn with_provider_identity(
        mut self,
        call_id: Option<String>,
        provider_item_id: Option<String>,
    ) -> Self {
        self.call_id = call_id;
        self.provider_item_id = provider_item_id;
        self
    }

    pub fn with_working_directory(mut self, working_directory: Option<String>) -> Self {
        self.working_directory = working_directory;
        self
    }

    pub fn tool_call_id(&self) -> &str {
        &self.tool_call_id
    }

    pub fn call_id(&self) -> Option<&str> {
        self.call_id.as_deref()
    }

    pub fn provider_item_id(&self) -> Option<&str> {
        self.provider_item_id.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arguments(&self) -> &str {
        &self.arguments
    }

    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceToolOutput {
    result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    output_artifacts: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    audit_metadata: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    metrics: Option<TraceToolOutputMetrics>,
}

impl TraceToolOutput {
    pub fn new(result: String) -> Self {
        Self {
            result,
            exit_code: None,
            output_artifacts: Vec::new(),
            audit_metadata: Vec::new(),
            metrics: None,
        }
    }

    pub fn with_details(
        mut self,
        exit_code: Option<i32>,
        output_artifacts: Vec<serde_json::Value>,
        audit_metadata: Vec<serde_json::Value>,
        metrics: Option<TraceToolOutputMetrics>,
    ) -> Self {
        self.exit_code = exit_code;
        self.output_artifacts = output_artifacts;
        self.audit_metadata = audit_metadata;
        self.metrics = metrics;
        self
    }

    pub fn result(&self) -> &str {
        &self.result
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub fn output_artifacts(&self) -> &[serde_json::Value] {
        &self.output_artifacts
    }

    pub fn audit_metadata(&self) -> &[serde_json::Value] {
        &self.audit_metadata
    }

    pub fn metrics(&self) -> Option<&TraceToolOutputMetrics> {
        self.metrics.as_ref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceToolOutputMetrics {
    pub raw_bytes: u64,
    pub model_visible_bytes: u64,
    pub artifact_bytes: u64,
    pub result_hash: String,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TraceToolState {
    Started(StartedTraceTool),
    Streaming(StreamingTraceTool),
    AwaitingApproval(AwaitingApprovalTraceTool),
    Approved(ApprovedTraceTool),
    Running(RunningTraceTool),
    Succeeded(SucceededTraceTool),
    Failed(FailedTraceTool),
    Denied(DeniedTraceTool),
    Cancelled(CancelledTraceTool),
}

impl TraceToolState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded(_) | Self::Failed(_) | Self::Denied(_) | Self::Cancelled(_)
        )
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(state.failure.message()),
            Self::Started(_)
            | Self::Streaming(_)
            | Self::AwaitingApproval(_)
            | Self::Approved(_)
            | Self::Running(_)
            | Self::Succeeded(_)
            | Self::Denied(_)
            | Self::Cancelled(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceToolActivePhase {
    AwaitingApproval,
    Approved,
    Running,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartedTraceTool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamingTraceTool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AwaitingApprovalTraceTool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovedTraceTool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningTraceTool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SucceededTraceTool {
    output: TraceToolOutput,
}

impl SucceededTraceTool {
    pub fn output(&self) -> &TraceToolOutput {
        &self.output
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedTraceTool {
    failure: TraceToolFailure,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<TraceToolOutput>,
}

impl FailedTraceTool {
    pub fn failure(&self) -> &TraceToolFailure {
        &self.failure
    }

    pub fn output(&self) -> Option<&TraceToolOutput> {
        self.output.as_ref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeniedTraceTool {
    reason: String,
}

impl DeniedTraceTool {
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledTraceTool {
    cause: TraceToolCancellationCause,
}

impl CancelledTraceTool {
    pub fn cause(&self) -> &TraceToolCancellationCause {
        &self.cause
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceToolFailure {
    kind: TraceToolFailureKind,
    message: String,
}

impl TraceToolFailure {
    pub fn new(kind: TraceToolFailureKind, message: String) -> Self {
        Self { kind, message }
    }

    pub fn kind(&self) -> TraceToolFailureKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TraceToolFailureKind {
    Execution,
    TimedOut,
    BudgetLimited,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TraceToolCancellationCause {
    TurnCancelled { reason: String },
    Superseded { replacement_item_id: String },
}

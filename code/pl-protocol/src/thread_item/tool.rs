use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadToolItem {
    invocation: ThreadToolInvocation,
    state: ThreadToolState,
}

impl ThreadToolItem {
    pub fn new(invocation: ThreadToolInvocation, state: ThreadToolState) -> Self {
        Self { invocation, state }
    }

    pub fn invocation(&self) -> &ThreadToolInvocation {
        &self.invocation
    }

    pub fn state(&self) -> &ThreadToolState {
        &self.state
    }

    pub fn terminal_output(&self) -> Option<&ThreadToolOutput> {
        match &self.state {
            ThreadToolState::Succeeded(state) => Some(&state.output),
            ThreadToolState::Failed(state) => state.output.as_ref(),
            ThreadToolState::Started(_)
            | ThreadToolState::Streaming(_)
            | ThreadToolState::AwaitingApproval(_)
            | ThreadToolState::Approved(_)
            | ThreadToolState::Running(_)
            | ThreadToolState::Denied(_)
            | ThreadToolState::Cancelled(_) => None,
        }
    }

    pub(super) fn append_arguments(&mut self, delta: &str) -> Result<(), &'static str> {
        match self.state {
            ThreadToolState::Started(_) | ThreadToolState::Streaming(_) => {
                self.invocation.arguments.push_str(delta);
                self.state = ThreadToolState::Streaming(StreamingThreadTool);
                Ok(())
            }
            ThreadToolState::AwaitingApproval(_)
            | ThreadToolState::Approved(_)
            | ThreadToolState::Running(_)
            | ThreadToolState::Succeeded(_)
            | ThreadToolState::Failed(_)
            | ThreadToolState::Denied(_)
            | ThreadToolState::Cancelled(_) => {
                Err("tool argument delta requires started or streaming state")
            }
        }
    }

    pub(super) fn append_result(&mut self, delta: &str) -> Result<(), &'static str> {
        match &mut self.state {
            ThreadToolState::Running(state) => {
                state.streamed_output.push_str(delta);
                Ok(())
            }
            ThreadToolState::Started(_)
            | ThreadToolState::Streaming(_)
            | ThreadToolState::AwaitingApproval(_)
            | ThreadToolState::Approved(_)
            | ThreadToolState::Succeeded(_)
            | ThreadToolState::Failed(_)
            | ThreadToolState::Denied(_)
            | ThreadToolState::Cancelled(_) => Err("tool result delta requires running state"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadToolInvocation {
    tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_item_id: Option<String>,
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    working_directory: Option<String>,
}

impl ThreadToolInvocation {
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
        self.call_id = call_id.filter(|value| !value.is_empty());
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ThreadToolState {
    Started(StartedThreadTool),
    Streaming(StreamingThreadTool),
    AwaitingApproval(AwaitingApprovalThreadTool),
    Approved(ApprovedThreadTool),
    Running(RunningThreadTool),
    Succeeded(SucceededThreadTool),
    Failed(FailedThreadTool),
    Denied(DeniedThreadTool),
    Cancelled(CancelledThreadTool),
}

impl ThreadToolState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded(_) | Self::Failed(_) | Self::Denied(_) | Self::Cancelled(_)
        )
    }

    pub fn terminal_at(&self) -> Option<i64> {
        match self {
            Self::Succeeded(state) => Some(state.completed_at),
            Self::Failed(state) => Some(state.failed_at),
            Self::Denied(state) => Some(state.denied_at),
            Self::Cancelled(state) => Some(state.cancelled_at),
            Self::Started(_)
            | Self::Streaming(_)
            | Self::AwaitingApproval(_)
            | Self::Approved(_)
            | Self::Running(_) => None,
        }
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(&state.failure.message),
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartedThreadTool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamingThreadTool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AwaitingApprovalThreadTool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovedThreadTool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningThreadTool {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    streamed_output: String,
}

impl RunningThreadTool {
    pub fn new(streamed_output: String) -> Self {
        Self { streamed_output }
    }

    pub fn streamed_output(&self) -> &str {
        &self.streamed_output
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SucceededThreadTool {
    completed_at: i64,
    output: ThreadToolOutput,
}

impl SucceededThreadTool {
    pub fn new(completed_at: i64, output: ThreadToolOutput) -> Self {
        Self {
            completed_at,
            output,
        }
    }

    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }

    pub fn output(&self) -> &ThreadToolOutput {
        &self.output
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FailedThreadTool {
    failed_at: i64,
    failure: ThreadToolFailure,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<ThreadToolOutput>,
}

impl FailedThreadTool {
    pub fn new(
        failed_at: i64,
        failure: ThreadToolFailure,
        output: Option<ThreadToolOutput>,
    ) -> Self {
        Self {
            failed_at,
            failure,
            output,
        }
    }

    pub fn failed_at(&self) -> i64 {
        self.failed_at
    }

    pub fn failure(&self) -> &ThreadToolFailure {
        &self.failure
    }

    pub fn output(&self) -> Option<&ThreadToolOutput> {
        self.output.as_ref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeniedThreadTool {
    denied_at: i64,
    reason: String,
}

impl DeniedThreadTool {
    pub fn new(denied_at: i64, reason: String) -> Self {
        Self { denied_at, reason }
    }

    pub fn denied_at(&self) -> i64 {
        self.denied_at
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledThreadTool {
    cancelled_at: i64,
    reason: String,
}

impl CancelledThreadTool {
    pub fn new(cancelled_at: i64, reason: String) -> Self {
        Self {
            cancelled_at,
            reason,
        }
    }

    pub fn cancelled_at(&self) -> i64 {
        self.cancelled_at
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadToolOutput {
    result: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    output_artifacts: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
}

impl ThreadToolOutput {
    pub fn new(
        result: String,
        output_artifacts: Vec<serde_json::Value>,
        exit_code: Option<i32>,
    ) -> Self {
        Self {
            result,
            output_artifacts,
            exit_code,
        }
    }

    pub fn result(&self) -> &str {
        &self.result
    }

    pub fn output_artifacts(&self) -> &[serde_json::Value] {
        &self.output_artifacts
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadToolFailure {
    kind: ThreadToolFailureKind,
    message: String,
}

impl ThreadToolFailure {
    pub fn new(kind: ThreadToolFailureKind, message: String) -> Self {
        Self { kind, message }
    }

    pub fn kind(&self) -> ThreadToolFailureKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadToolFailureKind {
    Execution,
    TimedOut,
    BudgetLimited,
}

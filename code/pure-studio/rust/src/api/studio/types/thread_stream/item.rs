//! Typed FRB projection of canonical Thread Item state machines.

use super::{BridgeTokenUsageSnapshot, BridgeTurnState};

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadItem {
    pub id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub ordinal: u64,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub state: BridgeThreadItemState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeThreadItemState {
    Text {
        channel: BridgeThreadTextChannel,
        text: String,
        attachments: Vec<BridgeThreadAttachment>,
        lifecycle: BridgeThreadContentLifecycle,
    },
    Thinking {
        summary: Vec<String>,
        content: Vec<String>,
        lifecycle: BridgeThreadContentLifecycle,
    },
    Tool {
        invocation: BridgeThreadToolInvocation,
        state: BridgeThreadToolState,
    },
    Agent {
        identity: BridgeThreadAgentIdentity,
        state: BridgeThreadAgentState,
    },
    Turn {
        state: BridgeTurnState,
    },
    Inference {
        inference_id: String,
        model: String,
        state: BridgeThreadInferenceState,
    },
    Plan {
        content: String,
        lifecycle: BridgeThreadContentLifecycle,
    },
    Skill {
        name: String,
        source: String,
        provider_id: String,
        resource_base: BridgeSkillResourceBase,
        cause: BridgeSkillActivationCause,
        activated_at: i64,
    },
    File {
        path: String,
        media_type: Option<String>,
        completed_at: i64,
    },
    ContextCompaction {
        before_tokens: u64,
        after_tokens: u64,
        compacted_at: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSkillResourceBase {
    Directory { path: String },
    Url { url: String },
    Opaque { description: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSkillActivationCause {
    Tool { tool_call_id: String },
    UserGesture { invocation_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadTextChannel {
    User,
    Commentary,
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeThreadContentLifecycle {
    Streaming,
    Completed { completed_at: i64 },
    Failed { failed_at: i64, error: String },
    Cancelled { cancelled_at: i64, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadAttachment {
    pub id: String,
    pub modality: crate::api::studio::types::BridgeAttachmentModality,
    pub media_type: String,
    pub filename: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadToolInvocation {
    pub tool_call_id: String,
    pub call_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub name: String,
    pub arguments: String,
    pub working_directory: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeThreadToolState {
    Started,
    Streaming,
    AwaitingApproval,
    Approved,
    Running {
        streamed_output: String,
    },
    Succeeded {
        completed_at: i64,
        output: BridgeThreadToolOutput,
    },
    Failed {
        failed_at: i64,
        failure: BridgeThreadToolFailure,
        output: Option<BridgeThreadToolOutput>,
    },
    Denied {
        denied_at: i64,
        reason: String,
    },
    Cancelled {
        cancelled_at: i64,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadToolOutput {
    pub result: String,
    pub output_artifacts_json: Vec<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadToolFailure {
    pub kind: BridgeThreadToolFailureKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadToolFailureKind {
    Execution,
    TimedOut,
    BudgetLimited,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadAgentIdentity {
    pub id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeThreadAgentState {
    Queued,
    Running,
    Succeeded { completed_at: i64, summary: String },
    Denied { denied_at: i64, reason: String },
    Cancelled { cancelled_at: i64, reason: String },
    Failed { failed_at: i64, error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeThreadInferenceState {
    Running,
    Completed {
        completed_at: i64,
        usage: BridgeTokenUsageSnapshot,
    },
    Failed {
        failed_at: i64,
        error: String,
    },
    Cancelled {
        cancelled_at: i64,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadItemDelta {
    pub item_id: String,
    pub revision: u64,
    pub delta: BridgeThreadItemDeltaState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeThreadItemDeltaState {
    Text { delta: String },
    ThinkingSummary { chunk_index: u32, delta: String },
    ThinkingContent { chunk_index: u32, delta: String },
    Plan { delta: String },
    ToolArguments { delta: String },
    ToolResult { delta: String },
}

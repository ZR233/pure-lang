//! Product projections of background tool tasks; wakeups use the core Thread inbox.

use serde::{Deserialize, Serialize};

/// The lifecycle of a session-owned tool invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolTaskStatus {
    Queued,
    WaitingApproval,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl ToolTaskStatus {
    /// Whether the task can no longer execute or change its outcome.
    pub fn is_terminal(self) -> bool {
        match self {
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted => true,
            Self::Queued | Self::WaitingApproval | Self::Running | Self::Cancelling => false,
        }
    }
}

/// Model-visible acknowledgement, not a successful execution result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskReceipt {
    pub status: ToolTaskAcceptance,
    pub task_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub call_id: String,
    pub item_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_generation: u64,
}

/// Acceptance is not an execution outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolTaskAcceptance {
    Accepted,
}

/// Delivery ownership is independent of physical execution status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolTaskDelivery {
    PendingResponse,
    DirectOffered,
    DirectCommitted,
    #[default]
    Background,
}

/// Canonical task state with a bounded model preview and selected delivery route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskSnapshot {
    pub receipt: ToolTaskReceipt,
    pub status: ToolTaskStatus,
    #[serde(default)]
    pub delivery: ToolTaskDelivery,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<ToolTaskResult>,
    /// Model-facing reference to the complete, host-audit-filtered result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_reference: Option<ToolTaskResultReference>,
}

/// Immutable result identity and the initial opaque cursor for model readback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskResultReference {
    pub content_hash: String,
    pub encoded_bytes: u64,
    /// False means fields were omitted from the inline preview; pages retain them.
    pub preview_complete: bool,
    pub cursor: String,
}

/// A UTF-8 segment of the complete model-facing result JSON, not a partial JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskResultPage {
    pub task_id: String,
    pub content_hash: String,
    pub offset: u64,
    pub encoded_bytes: u64,
    pub text: String,
    pub next_cursor: Option<String>,
}

/// Producer-owned complete task payload. Consumers interpret the format and version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskPayload {
    pub format: String,
    pub version: u32,
    pub content: String,
}

/// Final tool output, independent of the Turn that submitted the invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskResult {
    /// Exact model-facing text supplied by the producer.
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<ToolTaskPayload>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_activations: Vec<crate::SkillActivation>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::ThreadAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<ToolTaskOutputFact>,
}

/// Execution output metadata, not instructions to mutate a Turn or an executor handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToolTaskOutputFact {
    OutputArtifacts {
        artifacts: Vec<serde_json::Value>,
    },
    /// Retained for host inspection and persistence; excluded from model projections.
    AuditMetadata {
        metadata: serde_json::Value,
    },
    CacheHit {
        #[serde(rename = "reusedFromCallId")]
        reused_from_call_id: String,
        #[serde(rename = "resultHash")]
        result_hash: String,
        #[serde(rename = "totalBytes")]
        total_bytes: u64,
    },
    OutputMetrics {
        #[serde(rename = "rawBytes")]
        raw_bytes: u64,
        #[serde(rename = "modelVisibleBytes")]
        model_visible_bytes: u64,
        #[serde(rename = "artifactBytes")]
        artifact_bytes: u64,
        #[serde(rename = "resultHash")]
        result_hash: String,
    },
    OutputBudget {
        #[serde(rename = "maxBytes")]
        max_bytes: usize,
    },
    /// An asynchronous task attempted an operation reserved for a control tool.
    RejectedControl {
        directive: ToolTaskRejectedControl,
    },
}

/// Control operations that cannot be committed as ordinary asynchronous task results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolTaskRejectedControl {
    SessionEvents,
    InteractionRequested,
    RevealTools,
    EndTurn,
}

/// A bounded, non-consuming task query result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskPage {
    pub tasks: Vec<ToolTaskSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Compact task listing; complete output is obtained through get_tool_task result pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskSummary {
    pub receipt: ToolTaskReceipt,
    pub status: ToolTaskStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<&ToolTaskSnapshot> for ToolTaskSummary {
    fn from(task: &ToolTaskSnapshot) -> Self {
        Self {
            receipt: task.receipt.clone(),
            status: task.status,
            created_at: task.created_at,
            updated_at: task.updated_at,
        }
    }
}

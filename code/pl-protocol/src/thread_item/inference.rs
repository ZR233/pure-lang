use serde::{Deserialize, Serialize};

use crate::TokenUsageSnapshot;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadInferenceItem {
    inference_id: String,
    model: String,
    state: ThreadInferenceState,
}

impl ThreadInferenceItem {
    pub fn new(inference_id: String, model: String, state: ThreadInferenceState) -> Self {
        Self {
            inference_id,
            model,
            state,
        }
    }

    pub fn inference_id(&self) -> &str {
        &self.inference_id
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn state(&self) -> &ThreadInferenceState {
        &self.state
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ThreadInferenceState {
    Running(RunningThreadInference),
    Completed(CompletedThreadInference),
    Failed(FailedThreadInference),
    Cancelled(CancelledThreadInference),
}

impl ThreadInferenceState {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Running(_))
    }

    pub fn terminal_at(&self) -> Option<i64> {
        match self {
            Self::Running(_) => None,
            Self::Completed(state) => Some(state.completed_at),
            Self::Failed(state) => Some(state.failed_at),
            Self::Cancelled(state) => Some(state.cancelled_at),
        }
    }

    pub fn usage(&self) -> Option<&TokenUsageSnapshot> {
        match self {
            Self::Completed(state) => Some(&state.usage),
            Self::Running(_) | Self::Failed(_) | Self::Cancelled(_) => None,
        }
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(&state.error),
            Self::Running(_) | Self::Completed(_) | Self::Cancelled(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningThreadInference;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedThreadInference {
    completed_at: i64,
    usage: TokenUsageSnapshot,
}

impl CompletedThreadInference {
    pub fn new(completed_at: i64, usage: TokenUsageSnapshot) -> Self {
        Self {
            completed_at,
            usage,
        }
    }

    pub fn usage(&self) -> &TokenUsageSnapshot {
        &self.usage
    }

    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedThreadInference {
    failed_at: i64,
    error: String,
}

impl FailedThreadInference {
    pub fn new(failed_at: i64, error: String) -> Self {
        Self { failed_at, error }
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn failed_at(&self) -> i64 {
        self.failed_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledThreadInference {
    cancelled_at: i64,
    reason: String,
}

impl CancelledThreadInference {
    pub fn new(cancelled_at: i64, reason: String) -> Self {
        Self {
            cancelled_at,
            reason,
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn cancelled_at(&self) -> i64 {
        self.cancelled_at
    }
}

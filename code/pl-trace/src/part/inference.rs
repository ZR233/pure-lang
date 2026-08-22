use pl_protocol::TokenUsageSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceInferencePart {
    inference_id: String,
    model: String,
    state: TraceInferenceState,
}

impl TraceInferencePart {
    pub fn running(inference_id: String, model: String) -> Self {
        Self {
            inference_id,
            model,
            state: TraceInferenceState::Running(RunningTraceInference),
        }
    }

    pub fn inference_id(&self) -> &str {
        &self.inference_id
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn state(&self) -> &TraceInferenceState {
        &self.state
    }

    pub(super) fn set_model(&self, model: String) -> Result<Self, &'static str> {
        if !matches!(self.state, TraceInferenceState::Running(_)) {
            return Err("inference model update requires running state");
        }
        let mut next = self.clone();
        next.model = model;
        Ok(next)
    }

    pub(super) fn complete(&self, usage: TokenUsageSnapshot) -> Result<Self, &'static str> {
        if !matches!(self.state, TraceInferenceState::Running(_)) {
            return Err("inference completion requires running state");
        }
        let mut next = self.clone();
        next.state = TraceInferenceState::Completed(CompletedTraceInference { usage });
        Ok(next)
    }

    pub(super) fn fail(&self, error: String) -> Result<Self, &'static str> {
        if !matches!(self.state, TraceInferenceState::Running(_)) {
            return Err("inference failure requires running state");
        }
        let mut next = self.clone();
        next.state = TraceInferenceState::Failed(FailedTraceInference { error });
        Ok(next)
    }

    pub(super) fn cancel(&self, reason: String) -> Result<Self, &'static str> {
        if !matches!(self.state, TraceInferenceState::Running(_)) {
            return Err("inference cancellation requires running state");
        }
        let mut next = self.clone();
        next.state = TraceInferenceState::Cancelled(CancelledTraceInference { reason });
        Ok(next)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TraceInferenceState {
    Running(RunningTraceInference),
    Completed(CompletedTraceInference),
    Failed(FailedTraceInference),
    Cancelled(CancelledTraceInference),
}

impl TraceInferenceState {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Running(_))
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
pub struct RunningTraceInference;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedTraceInference {
    usage: TokenUsageSnapshot,
}

impl CompletedTraceInference {
    pub fn usage(&self) -> &TokenUsageSnapshot {
        &self.usage
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedTraceInference {
    error: String,
}

impl FailedTraceInference {
    pub fn error(&self) -> &str {
        &self.error
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledTraceInference {
    reason: String,
}

impl CancelledTraceInference {
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

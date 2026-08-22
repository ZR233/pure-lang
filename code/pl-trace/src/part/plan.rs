use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TracePlanPart {
    content: String,
    state: TracePlanState,
}

impl TracePlanPart {
    pub fn started() -> Self {
        Self {
            content: String::new(),
            state: TracePlanState::Started(StartedTracePlan),
        }
    }

    pub fn streaming() -> Self {
        Self {
            content: String::new(),
            state: TracePlanState::Streaming(StreamingTracePlan),
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn state(&self) -> &TracePlanState {
        &self.state
    }

    pub(super) fn append(&self, delta: &str) -> Result<Self, &'static str> {
        if !matches!(
            self.state,
            TracePlanState::Started(_) | TracePlanState::Streaming(_)
        ) {
            return Err("plan delta requires an open state");
        }
        let mut next = self.clone();
        next.content.push_str(delta);
        next.state = TracePlanState::Streaming(StreamingTracePlan);
        Ok(next)
    }

    pub(super) fn complete(&self, content: Option<String>) -> Result<Self, &'static str> {
        if !matches!(
            self.state,
            TracePlanState::Started(_) | TracePlanState::Streaming(_)
        ) {
            return Err("plan completion requires an open state");
        }
        let mut next = self.clone();
        if let Some(content) = content {
            next.content = content;
        }
        next.state = TracePlanState::Completed(CompletedTracePlan);
        Ok(next)
    }

    pub(super) fn fail(&self, error: String) -> Result<Self, &'static str> {
        if !matches!(
            self.state,
            TracePlanState::Started(_) | TracePlanState::Streaming(_)
        ) {
            return Err("plan failure requires an open state");
        }
        let mut next = self.clone();
        next.state = TracePlanState::Failed(FailedTracePlan { error });
        Ok(next)
    }

    pub(super) fn cancel(&self, reason: String) -> Result<Self, &'static str> {
        if !matches!(
            self.state,
            TracePlanState::Started(_) | TracePlanState::Streaming(_)
        ) {
            return Err("plan cancellation requires an open state");
        }
        let mut next = self.clone();
        next.state = TracePlanState::Cancelled(CancelledTracePlan { reason });
        Ok(next)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TracePlanState {
    Started(StartedTracePlan),
    Streaming(StreamingTracePlan),
    Completed(CompletedTracePlan),
    Failed(FailedTracePlan),
    Cancelled(CancelledTracePlan),
}

impl TracePlanState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed(_) | Self::Failed(_) | Self::Cancelled(_)
        )
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(&state.error),
            Self::Started(_) | Self::Streaming(_) | Self::Completed(_) | Self::Cancelled(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartedTracePlan;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamingTracePlan;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedTracePlan;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedTracePlan {
    error: String,
}

impl FailedTracePlan {
    pub fn error(&self) -> &str {
        &self.error
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledTracePlan {
    reason: String,
}

impl CancelledTracePlan {
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

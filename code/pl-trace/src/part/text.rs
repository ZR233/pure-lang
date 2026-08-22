use serde::{Deserialize, Serialize};

use super::TraceAttachment;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceTextPart {
    channel: TraceTextChannel,
    content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<TraceAttachment>,
    state: TraceTextState,
}

impl TraceTextPart {
    pub fn streaming(channel: TraceTextChannel, content: String) -> Self {
        Self {
            channel,
            content,
            attachments: Vec::new(),
            state: TraceTextState::Streaming(StreamingTraceText),
        }
    }

    pub fn completed(
        channel: TraceTextChannel,
        content: String,
        attachments: Vec<TraceAttachment>,
    ) -> Self {
        Self {
            channel,
            content,
            attachments,
            state: TraceTextState::Completed(CompletedTraceText),
        }
    }

    pub fn channel(&self) -> TraceTextChannel {
        self.channel
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn attachments(&self) -> &[TraceAttachment] {
        &self.attachments
    }

    pub fn state(&self) -> &TraceTextState {
        &self.state
    }

    pub(super) fn append(&self, delta: &str) -> Result<Self, &'static str> {
        if !matches!(self.state, TraceTextState::Streaming(_)) {
            return Err("text delta requires streaming state");
        }
        let mut next = self.clone();
        next.content.push_str(delta);
        Ok(next)
    }

    pub(super) fn complete(
        &self,
        authoritative_content: Option<String>,
    ) -> Result<Self, &'static str> {
        if !matches!(self.state, TraceTextState::Streaming(_)) {
            return Err("text completion requires streaming state");
        }
        let mut next = self.clone();
        if let Some(content) = authoritative_content {
            next.content = content;
        }
        next.state = TraceTextState::Completed(CompletedTraceText);
        Ok(next)
    }

    pub(super) fn fail(&self, error: String) -> Result<Self, &'static str> {
        if !matches!(self.state, TraceTextState::Streaming(_)) {
            return Err("text failure requires streaming state");
        }
        let mut next = self.clone();
        next.state = TraceTextState::Failed(FailedTraceText { error });
        Ok(next)
    }

    pub(super) fn cancel(&self, reason: String) -> Result<Self, &'static str> {
        if !matches!(self.state, TraceTextState::Streaming(_)) {
            return Err("text cancellation requires streaming state");
        }
        let mut next = self.clone();
        next.state = TraceTextState::Cancelled(CancelledTraceText { reason });
        Ok(next)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TraceTextChannel {
    User,
    Commentary,
    Final,
}

impl TraceTextChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Commentary => "commentary",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TraceTextState {
    Streaming(StreamingTraceText),
    Completed(CompletedTraceText),
    Failed(FailedTraceText),
    Cancelled(CancelledTraceText),
}

impl TraceTextState {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Streaming(_))
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(&state.error),
            Self::Streaming(_) | Self::Completed(_) | Self::Cancelled(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamingTraceText;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedTraceText;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedTraceText {
    error: String,
}

impl FailedTraceText {
    pub fn error(&self) -> &str {
        &self.error
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledTraceText {
    reason: String,
}

impl CancelledTraceText {
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

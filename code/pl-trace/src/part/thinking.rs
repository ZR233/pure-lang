use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceThinkingPart {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    summary: Vec<TraceThinkingChunk>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    content: Vec<TraceThinkingChunk>,
    state: TraceThinkingState,
}

impl TraceThinkingPart {
    pub fn streaming() -> Self {
        Self {
            summary: Vec::new(),
            content: Vec::new(),
            state: TraceThinkingState::Streaming(StreamingTraceThinking),
        }
    }

    pub fn summary(&self) -> &[TraceThinkingChunk] {
        &self.summary
    }

    pub fn content(&self) -> &[TraceThinkingChunk] {
        &self.content
    }

    pub fn state(&self) -> &TraceThinkingState {
        &self.state
    }

    pub(super) fn append_summary(
        &self,
        chunk_index: u32,
        delta: &str,
    ) -> Result<Self, &'static str> {
        let mut next = self.require_streaming()?;
        append_chunk(&mut next.summary, chunk_index, delta);
        Ok(next)
    }

    pub(super) fn append_content(
        &self,
        chunk_index: u32,
        delta: &str,
    ) -> Result<Self, &'static str> {
        let mut next = self.require_streaming()?;
        append_chunk(&mut next.content, chunk_index, delta);
        Ok(next)
    }

    pub(super) fn complete(
        &self,
        authoritative_summary: Option<Vec<String>>,
    ) -> Result<Self, &'static str> {
        let mut next = self.require_streaming()?;
        if let Some(summary) = authoritative_summary {
            next.summary = summary
                .into_iter()
                .enumerate()
                .map(|(index, content)| TraceThinkingChunk {
                    chunk_index: index as u32,
                    content,
                })
                .collect();
        }
        next.state = TraceThinkingState::Completed(CompletedTraceThinking);
        Ok(next)
    }

    pub(super) fn fail(&self, error: String) -> Result<Self, &'static str> {
        let mut next = self.require_streaming()?;
        next.state = TraceThinkingState::Failed(FailedTraceThinking { error });
        Ok(next)
    }

    pub(super) fn cancel(&self, reason: String) -> Result<Self, &'static str> {
        let mut next = self.require_streaming()?;
        next.state = TraceThinkingState::Cancelled(CancelledTraceThinking { reason });
        Ok(next)
    }

    fn require_streaming(&self) -> Result<Self, &'static str> {
        matches!(self.state, TraceThinkingState::Streaming(_))
            .then(|| self.clone())
            .ok_or("thinking mutation requires streaming state")
    }
}

fn append_chunk(chunks: &mut Vec<TraceThinkingChunk>, chunk_index: u32, delta: &str) {
    match chunks
        .iter_mut()
        .find(|chunk| chunk.chunk_index == chunk_index)
    {
        Some(chunk) => chunk.content.push_str(delta),
        None => chunks.push(TraceThinkingChunk {
            chunk_index,
            content: delta.to_string(),
        }),
    }
    chunks.sort_by_key(|chunk| chunk.chunk_index);
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceThinkingChunk {
    pub chunk_index: u32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TraceThinkingState {
    Streaming(StreamingTraceThinking),
    Completed(CompletedTraceThinking),
    Failed(FailedTraceThinking),
    Cancelled(CancelledTraceThinking),
}

impl TraceThinkingState {
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
pub struct StreamingTraceThinking;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedTraceThinking;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedTraceThinking {
    error: String,
}

impl FailedTraceThinking {
    pub fn error(&self) -> &str {
        &self.error
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledTraceThinking {
    reason: String,
}

impl CancelledTraceThinking {
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

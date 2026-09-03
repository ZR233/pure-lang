use serde::{Deserialize, Serialize};

use super::ThreadAttachment;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTextItem {
    channel: ThreadTextChannel,
    text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<ThreadAttachment>,
    lifecycle: ThreadContentLifecycle,
}

impl ThreadTextItem {
    pub fn new(
        channel: ThreadTextChannel,
        text: String,
        attachments: Vec<ThreadAttachment>,
        lifecycle: ThreadContentLifecycle,
    ) -> Self {
        Self {
            channel,
            text,
            attachments,
            lifecycle,
        }
    }

    pub fn channel(&self) -> ThreadTextChannel {
        self.channel
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn attachments(&self) -> &[ThreadAttachment] {
        &self.attachments
    }

    pub fn lifecycle(&self) -> &ThreadContentLifecycle {
        &self.lifecycle
    }

    pub(super) fn append(&mut self, delta: &str) -> Result<(), &'static str> {
        self.lifecycle.require_streaming()?;
        self.text.push_str(delta);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadTextChannel {
    User,
    /// 由当前 Thread 的直接父代理提交的输入。
    ParentAgent,
    Commentary,
    Final,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadThinkingItem {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    content: Vec<String>,
    lifecycle: ThreadContentLifecycle,
}

impl ThreadThinkingItem {
    pub fn new(
        summary: Vec<String>,
        content: Vec<String>,
        lifecycle: ThreadContentLifecycle,
    ) -> Self {
        Self {
            summary,
            content,
            lifecycle,
        }
    }

    pub fn summary(&self) -> &[String] {
        &self.summary
    }

    pub fn content(&self) -> &[String] {
        &self.content
    }

    pub fn lifecycle(&self) -> &ThreadContentLifecycle {
        &self.lifecycle
    }

    pub(super) fn append_summary(
        &mut self,
        chunk_index: u32,
        delta: &str,
    ) -> Result<(), &'static str> {
        self.lifecycle.require_streaming()?;
        append_chunk(&mut self.summary, chunk_index, delta)
    }

    pub(super) fn append_content(
        &mut self,
        chunk_index: u32,
        delta: &str,
    ) -> Result<(), &'static str> {
        self.lifecycle.require_streaming()?;
        append_chunk(&mut self.content, chunk_index, delta)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ThreadContentLifecycle {
    Streaming(StreamingThreadContent),
    Completed(CompletedThreadContent),
    Failed(FailedThreadContent),
    Cancelled(CancelledThreadContent),
}

impl ThreadContentLifecycle {
    pub fn streaming() -> Self {
        Self::Streaming(StreamingThreadContent)
    }

    pub fn completed(completed_at: i64) -> Self {
        Self::Completed(CompletedThreadContent { completed_at })
    }

    pub fn failed(failed_at: i64, error: String) -> Self {
        Self::Failed(FailedThreadContent { failed_at, error })
    }

    pub fn cancelled(cancelled_at: i64, reason: String) -> Self {
        Self::Cancelled(CancelledThreadContent {
            cancelled_at,
            reason,
        })
    }

    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Streaming(_))
    }

    pub fn terminal_at(&self) -> Option<i64> {
        match self {
            Self::Streaming(_) => None,
            Self::Completed(state) => Some(state.completed_at),
            Self::Failed(state) => Some(state.failed_at),
            Self::Cancelled(state) => Some(state.cancelled_at),
        }
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(&state.error),
            Self::Streaming(_) | Self::Completed(_) | Self::Cancelled(_) => None,
        }
    }

    pub fn cancellation_reason(&self) -> Option<&str> {
        match self {
            Self::Cancelled(state) => Some(&state.reason),
            Self::Streaming(_) | Self::Completed(_) | Self::Failed(_) => None,
        }
    }

    fn require_streaming(&self) -> Result<(), &'static str> {
        if matches!(self, Self::Streaming(_)) {
            Ok(())
        } else {
            Err("content delta requires streaming lifecycle")
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamingThreadContent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedThreadContent {
    completed_at: i64,
}

impl CompletedThreadContent {
    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedThreadContent {
    failed_at: i64,
    error: String,
}

impl FailedThreadContent {
    pub fn failed_at(&self) -> i64 {
        self.failed_at
    }

    pub fn error(&self) -> &str {
        &self.error
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledThreadContent {
    cancelled_at: i64,
    reason: String,
}

impl CancelledThreadContent {
    pub fn cancelled_at(&self) -> i64 {
        self.cancelled_at
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadFileItem {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
    completed_at: i64,
}

impl ThreadFileItem {
    pub fn new(path: String, media_type: Option<String>, completed_at: i64) -> Self {
        Self {
            path,
            media_type,
            completed_at,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }

    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadContextCompactionItem {
    before_tokens: u64,
    after_tokens: u64,
    compacted_at: i64,
}

impl ThreadContextCompactionItem {
    pub fn new(before_tokens: u64, after_tokens: u64, compacted_at: i64) -> Self {
        Self {
            before_tokens,
            after_tokens,
            compacted_at,
        }
    }

    pub fn before_tokens(&self) -> u64 {
        self.before_tokens
    }

    pub fn after_tokens(&self) -> u64 {
        self.after_tokens
    }

    pub fn compacted_at(&self) -> i64 {
        self.compacted_at
    }
}

fn append_chunk(
    chunks: &mut Vec<String>,
    chunk_index: u32,
    delta: &str,
) -> Result<(), &'static str> {
    let index = usize::try_from(chunk_index).map_err(|_| "chunk index does not fit usize")?;
    if index > chunks.len() {
        return Err("chunk index skipped an earlier chunk");
    }
    if index == chunks.len() {
        chunks.push(String::new());
    }
    chunks[index].push_str(delta);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_agent_channel_uses_the_v10_wire_label() {
        assert_eq!(
            serde_json::to_value(ThreadTextChannel::ParentAgent).unwrap(),
            serde_json::json!("parentAgent")
        );
    }
}

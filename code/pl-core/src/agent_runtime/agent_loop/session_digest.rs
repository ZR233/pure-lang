use std::collections::BTreeSet;

use pl_protocol::{MessageContent, MessageRole, TOOL_CALLS_METADATA_KEY, ToolResultMetadata};

use super::super::{
    AgentRuntimeHost, AgentRuntimeResult, AgentSessionDigest, AgentSessionDigestMessage,
    AgentSessionDigestRole, AgentSessionState,
};
use super::AgentLoop;

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) fn read_session(&self) -> AgentRuntimeResult<AgentSessionDigest> {
        Ok(session_digest(&self.state.session))
    }
}

fn session_digest(session: &AgentSessionState) -> AgentSessionDigest {
    const MAX_MESSAGES: usize = 12;
    const MAX_TEXT_CHARS: usize = 6_000;

    let mut tool_names = BTreeSet::new();
    for message in session.session.messages() {
        if let Some(tool_calls) = message.metadata.get(TOOL_CALLS_METADATA_KEY)
            && let Ok(tool_calls) = serde_json::from_str::<Vec<pl_model::ToolCall>>(tool_calls)
        {
            tool_names.extend(tool_calls.into_iter().map(|tool| tool.name));
        }
        if message.role == MessageRole::Tool
            && let Ok(metadata) = ToolResultMetadata::from_metadata(&message.metadata)
            && !metadata.tool_name.is_empty()
        {
            tool_names.insert(metadata.tool_name);
        }
    }

    let eligible = session
        .session
        .messages()
        .iter()
        .filter(|message| matches!(message.role, MessageRole::User | MessageRole::Assistant))
        .count();
    let mut remaining = MAX_TEXT_CHARS;
    let mut truncated = eligible > MAX_MESSAGES;
    let mut messages = Vec::new();
    for message in session
        .session
        .messages()
        .iter()
        .rev()
        .filter(|message| matches!(message.role, MessageRole::User | MessageRole::Assistant))
        .take(MAX_MESSAGES)
    {
        let text = message_text(&message.content);
        if remaining == 0 {
            truncated = true;
            break;
        }
        let text_chars = text.chars().count();
        let text = if text_chars > remaining {
            truncated = true;
            text.chars().take(remaining).collect()
        } else {
            text
        };
        remaining = remaining.saturating_sub(text.chars().count());
        messages.push(AgentSessionDigestMessage {
            role: match message.role {
                MessageRole::User => AgentSessionDigestRole::User,
                MessageRole::Assistant => AgentSessionDigestRole::Assistant,
                MessageRole::System | MessageRole::Tool => unreachable!("filtered above"),
            },
            text,
        });
    }
    messages.reverse();
    AgentSessionDigest {
        through_sequence: session.session_event_sequence,
        truncated,
        messages,
        tool_names: tool_names.into_iter().collect(),
    }
}

fn message_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .filter_map(|part| match part {
                pl_protocol::ContentPart::Text { text } => Some(text.as_str()),
                pl_protocol::ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

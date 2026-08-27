use std::collections::BTreeSet;

use pl_protocol::{MessageContent, MessageRole};

use super::super::{
    AgentRuntimeHost, AgentRuntimeResult, AgentSessionDigest, AgentSessionDigestMessage,
    AgentSessionDigestRole, ThreadContextState,
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

fn session_digest(session: &ThreadContextState) -> AgentSessionDigest {
    const MAX_MESSAGES: usize = 12;
    const MAX_TEXT_CHARS: usize = 6_000;

    let mut tool_names = BTreeSet::new();
    for message in session.session.messages() {
        tool_names.extend(
            message
                .tool_calls
                .iter()
                .flatten()
                .map(|tool_call| tool_call.name.clone()),
        );
        if message.role == MessageRole::Tool
            && let Some(record) = &message.tool_result
            && !record.name.is_empty()
        {
            tool_names.insert(record.name.clone());
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
        through_sequence: session.thread_revision,
        truncated,
        messages,
        tool_names: tool_names.into_iter().collect(),
    }
}

fn message_text(content: &MessageContent) -> String {
    content.text_value()
}

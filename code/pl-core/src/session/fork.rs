use std::collections::HashMap;

use pl_protocol::{Message, MessageRole, TOOL_CALLS_METADATA_KEY};

pub(super) fn forkable_messages(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|message| match message.role {
            MessageRole::System | MessageRole::User => true,
            MessageRole::Assistant => !message.metadata.contains_key(TOOL_CALLS_METADATA_KEY),
            MessageRole::Tool => false,
        })
        .map(|message| Message {
            role: message.role,
            content: message.content.clone(),
            reasoning_content: None,
            metadata: HashMap::new(),
        })
        .collect()
}

pub(super) fn last_user_turns(messages: Vec<Message>, turns: usize) -> Vec<Message> {
    let (system, conversation): (Vec<_>, Vec<_>) = messages
        .into_iter()
        .partition(|message| message.role == MessageRole::System);
    let start = conversation
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, message)| message.role == MessageRole::User)
        .nth(turns.saturating_sub(1))
        .map_or(0, |(index, _)| index);
    system
        .into_iter()
        .chain(conversation.into_iter().skip(start))
        .collect()
}

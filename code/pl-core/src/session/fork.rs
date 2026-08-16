use std::collections::HashMap;

use pl_protocol::{Message, MessageRole};

pub(super) fn forkable_messages(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|message| match message.role {
            MessageRole::System | MessageRole::User => true,
            MessageRole::Assistant => message.tool_calls.is_none(),
            MessageRole::Tool => false,
        })
        .map(|message| Message {
            role: message.role,
            content: message.content.clone(),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
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

use std::collections::HashMap;

use pl_protocol::{
    ContentPart, Message, MessageContent, MessageRole, ModelContextItem,
    TOOL_CALL_CALL_ID_METADATA_KEY, TOOL_CALL_ID_METADATA_KEY,
};

use super::{
    APPROX_CHARS_PER_TOKEN, CompactionTrigger, ContextCompactionConfig,
    ContextCompactionReplacement, RecentInteractionTailConfig, SUMMARY_METADATA_KEY,
    SUMMARY_METADATA_VALUE,
};

pub(super) fn has_compactable_history(
    items: &[ModelContextItem],
    trigger: CompactionTrigger,
) -> bool {
    let raw_messages = items
        .iter()
        .filter_map(ModelContextItem::as_message)
        .filter(|message| !is_compaction_summary(message))
        .count();
    match trigger {
        CompactionTrigger::Manual | CompactionTrigger::WallClockRollover => raw_messages > 0,
        CompactionTrigger::EstimatedTokens | CompactionTrigger::ProviderPromptTokens(_) => {
            raw_messages > 1
        }
    }
}

pub(crate) fn build_compacted_history(
    messages: &[Message],
    summary: &str,
    config: &ContextCompactionConfig,
) -> Vec<Message> {
    match &config.replacement {
        ContextCompactionReplacement::SummaryThenRecentUsers { token_budget } => {
            let mut compacted = Vec::new();
            compacted.push(summary_message(summary, &config.summary_prefix));
            compacted.extend(recent_user_messages(messages, *token_budget));
            compacted
        }
        ContextCompactionReplacement::RecentUsersThenSummary { token_budget } => {
            let mut compacted = recent_user_messages(messages, *token_budget);
            compacted.push(summary_message(summary, &config.summary_prefix));
            compacted
        }
        ContextCompactionReplacement::RecentInteractionTail(tail) => {
            let mut compacted =
                recent_user_message_texts(messages, tail.max_user_chars, &config.summary_prefix)
                    .into_iter()
                    .map(user_text_message)
                    .collect::<Vec<_>>();
            compacted.extend(recent_interaction_tail(
                messages,
                &config.summary_prefix,
                tail,
            ));
            compacted.push(summary_message(summary, &config.summary_prefix));
            compacted
        }
    }
}

fn summary_message(summary: &str, summary_prefix: &str) -> Message {
    let mut metadata = HashMap::new();
    metadata.insert(
        SUMMARY_METADATA_KEY.to_string(),
        SUMMARY_METADATA_VALUE.to_string(),
    );
    let trimmed = summary.trim();
    Message {
        role: MessageRole::User,
        content: MessageContent::Text(format!("{summary_prefix}\n\n{trimmed}")),
        reasoning_content: None,
        metadata,
    }
}

pub(super) fn recent_user_messages(messages: &[Message], token_budget: u64) -> Vec<Message> {
    let mut selected = Vec::new();
    let mut remaining = token_budget;
    for message in messages.iter().rev() {
        if message.role != MessageRole::User || is_compaction_summary(message) || remaining == 0 {
            continue;
        }
        let message_tokens = estimate_message_tokens(message).max(1);
        if message_tokens <= remaining {
            selected.push(message.clone());
            remaining = remaining.saturating_sub(message_tokens);
        } else if let Some(message) = truncate_message_to_token_budget(message, remaining) {
            selected.push(message);
            remaining = 0;
        }
    }
    selected.reverse();
    selected
}

fn truncate_message_to_token_budget(message: &Message, max_tokens: u64) -> Option<Message> {
    if max_tokens == 0 {
        return None;
    }
    let max_chars = max_tokens.saturating_mul(APPROX_CHARS_PER_TOKEN) as usize;
    let mut message = message.clone();
    message.content = match message.content {
        MessageContent::Text(text) => MessageContent::Text(take_last_chars(&text, max_chars)),
        MessageContent::MultiPart(parts) => {
            let mut remaining = max_chars;
            let mut retained = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } if remaining > 0 => {
                        let text = if text.chars().count() <= remaining {
                            text
                        } else {
                            take_last_chars(&text, remaining)
                        };
                        remaining = remaining.saturating_sub(text.chars().count());
                        retained.push(ContentPart::Text { text });
                    }
                    ContentPart::Image { .. } => retained.push(part),
                    _ => {}
                }
            }
            if retained.is_empty() {
                return None;
            }
            MessageContent::MultiPart(retained)
        }
    };
    Some(message)
}

fn recent_user_message_texts(
    messages: &[Message],
    max_chars: usize,
    summary_prefix: &str,
) -> Vec<String> {
    let mut selected = Vec::new();
    let mut remaining = max_chars;
    for message in messages.iter().rev() {
        if remaining == 0 {
            break;
        }
        let Some(text) = user_message_text(message) else {
            continue;
        };
        if is_summary_text(text.trim(), summary_prefix) {
            continue;
        }
        let char_count = text.chars().count();
        if char_count <= remaining {
            selected.push(text.to_string());
            remaining = remaining.saturating_sub(char_count);
        } else {
            selected.push(take_last_chars(text, remaining));
            break;
        }
    }
    selected.reverse();
    selected
}

fn recent_interaction_tail(
    messages: &[Message],
    summary_prefix: &str,
    config: &RecentInteractionTailConfig,
) -> Vec<Message> {
    let mut selected = Vec::new();
    let mut assistant_items = 0;
    let mut tool_output_items = 0;
    for message in messages.iter().rev() {
        match message.role {
            MessageRole::Assistant => {
                if assistant_items >= config.assistant_items {
                    continue;
                }
                let text = message_text(message);
                if text.trim().is_empty() {
                    continue;
                }
                selected.push(assistant_text_message(take_last_chars(
                    &text,
                    config.max_assistant_chars,
                )));
                assistant_items += 1;
            }
            MessageRole::Tool => {
                if tool_output_items >= config.tool_output_items {
                    continue;
                }
                let call_id = message
                    .metadata
                    .get(TOOL_CALL_CALL_ID_METADATA_KEY)
                    .or_else(|| message.metadata.get(TOOL_CALL_ID_METADATA_KEY))
                    .map(String::as_str)
                    .unwrap_or("unknown");
                selected.push(user_text_message(format!(
                    "Recent tool result `{call_id}` retained for context checkpoint:\n{}",
                    compact_tool_output(&message_text(message), config.max_tool_output_chars)
                )));
                tool_output_items += 1;
            }
            MessageRole::User => {
                let Some(text) = user_message_text(message) else {
                    continue;
                };
                if is_summary_text(text.trim(), summary_prefix) {
                    break;
                }
            }
            MessageRole::System => {}
        }
    }
    selected.reverse();
    selected
}

fn user_text_message(text: impl Into<String>) -> Message {
    Message {
        role: MessageRole::User,
        content: MessageContent::Text(text.into()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

fn assistant_text_message(text: impl Into<String>) -> Message {
    Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(text.into()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

fn user_message_text(message: &Message) -> Option<&str> {
    if message.role != MessageRole::User {
        return None;
    }
    match &message.content {
        MessageContent::Text(text) => Some(text.as_str()),
        MessageContent::MultiPart(parts) => parts.iter().find_map(|part| match part {
            ContentPart::Text { text } => Some(text.as_str()),
            ContentPart::Image { .. } => None,
        }),
    }
}

pub(super) fn message_text(message: &Message) -> String {
    match &message.content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn compact_tool_output(output: &str, max_chars: usize) -> String {
    if output.chars().count() <= max_chars {
        return output.to_string();
    }
    let tail = take_last_chars(output, max_chars);
    format!("tool output truncated for context compaction; kept last {max_chars} chars\n{tail}")
}

fn take_last_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut chars = text.chars().rev().take(max_chars).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

pub(super) fn is_compaction_summary(message: &Message) -> bool {
    message
        .metadata
        .get(SUMMARY_METADATA_KEY)
        .is_some_and(|value| value == SUMMARY_METADATA_VALUE)
}

fn is_summary_text(text: &str, summary_prefix: &str) -> bool {
    text.starts_with(summary_prefix)
}

pub(super) fn estimate_message_tokens(message: &Message) -> u64 {
    match &message.content {
        MessageContent::Text(text) => estimate_text_tokens(text),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .map(|part| match part {
                ContentPart::Text { text } => estimate_text_tokens(text),
                ContentPart::Image { .. } => 0,
            })
            .sum(),
    }
}

pub(super) fn estimate_text_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    if chars == 0 {
        0
    } else {
        chars.div_ceil(APPROX_CHARS_PER_TOKEN)
    }
}

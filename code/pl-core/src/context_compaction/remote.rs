use pl_model::{
    ModelCompactionRequest, ModelProvider, OpenAiCompactionMode, ReasoningConfig, TokenUsage,
    ToolSchema,
};
use pl_protocol::{
    ContentPart, Message, MessageContent, MessageRole, ModelContextItem, PureError, Result,
};

use super::history::{estimate_message_tokens, is_compaction_summary};
use super::{APPROX_CHARS_PER_TOKEN, ContextCompactionConfig};
use crate::session::AgentSession;

const RETAINED_REMOTE_V2_TOKEN_BUDGET: u64 = 64_000;
const CONTEXT_WINDOW_TRUNCATED_OUTPUT_MESSAGE: &str =
    "Output exceeded the available model context and was truncated";

pub(super) struct RemoteCompactionRequest<'a, P: ModelProvider + ?Sized> {
    pub provider: &'a P,
    pub model: &'a str,
    pub config: &'a ContextCompactionConfig,
    pub request_instructions: &'a str,
    pub request_messages: &'a [Message],
    pub tools: &'a [ToolSchema],
    pub parallel_tool_calls: bool,
    pub reasoning: Option<ReasoningConfig>,
    pub prompt_cache_key: Option<String>,
}

pub(super) async fn compact_remote(
    session: &AgentSession,
    request: RemoteCompactionRequest<'_, impl ModelProvider + ?Sized>,
) -> Result<(Vec<ModelContextItem>, Option<TokenUsage>)> {
    let RemoteCompactionRequest {
        provider,
        model,
        config,
        request_instructions,
        request_messages,
        tools,
        parallel_tool_calls,
        reasoning,
        prompt_cache_key,
    } = request;
    let mut input = request_messages
        .iter()
        .cloned()
        .map(ModelContextItem::from)
        .chain(session.items().iter().cloned())
        .collect::<Vec<_>>();
    trim_tool_outputs_to_context_window(
        &mut input,
        request_instructions,
        provider.model_info(model).resolved_context_window(),
    );
    let response = provider
        .compact_context(ModelCompactionRequest {
            mode: config.openai_mode,
            model: model.to_string(),
            instructions: request_instructions.to_string(),
            input,
            tools: tools.to_vec(),
            parallel_tool_calls,
            reasoning,
            prompt_cache_key,
        })
        .await?;
    let replacement = match config.openai_mode {
        OpenAiCompactionMode::RemoteV2 => build_v2_replacement(session.messages(), response.input)?,
        OpenAiCompactionMode::RemoteLegacy => {
            filter_legacy_replacement(session.messages(), response.input)?
        }
        OpenAiCompactionMode::Local => {
            return Err(PureError::ConfigError(
                "remote compaction received local mode".to_string(),
            ));
        }
    };
    Ok((replacement, response.usage))
}

fn build_v2_replacement(
    messages: &[Message],
    output: Vec<ModelContextItem>,
) -> Result<Vec<ModelContextItem>> {
    let mut compactions = output
        .into_iter()
        .filter(ModelContextItem::is_compaction)
        .collect::<Vec<_>>();
    if compactions.len() != 1 {
        return Err(PureError::LlmError(format!(
            "remote compaction v2 expected one checkpoint, got {}",
            compactions.len()
        )));
    }
    let compaction = compactions.remove(0);
    let mut retained = retain_recent_user_messages(messages, RETAINED_REMOTE_V2_TOKEN_BUDGET)
        .into_iter()
        .map(ModelContextItem::from)
        .collect::<Vec<_>>();
    retained.push(compaction);
    Ok(retained)
}

fn filter_legacy_replacement(
    original_messages: &[Message],
    output: Vec<ModelContextItem>,
) -> Result<Vec<ModelContextItem>> {
    let canonical_users = original_messages
        .iter()
        .filter(|message| message.role == MessageRole::User && !is_compaction_summary(message))
        .map(|message| message.content.clone())
        .collect::<Vec<_>>();
    let replacement = output
        .into_iter()
        .filter(|item| match item {
            ModelContextItem::Compaction { .. } => true,
            ModelContextItem::Message { message } => match message.role {
                MessageRole::User => canonical_users.contains(&message.content),
                MessageRole::Assistant => true,
                MessageRole::System | MessageRole::Tool => false,
            },
        })
        .collect::<Vec<_>>();
    if !replacement.iter().any(ModelContextItem::is_compaction) {
        return Err(PureError::LlmError(
            "OpenAI compact endpoint returned no compaction checkpoint".to_string(),
        ));
    }
    Ok(replacement)
}

fn retain_recent_user_messages(messages: &[Message], max_tokens: u64) -> Vec<Message> {
    let mut remaining = max_tokens;
    let mut retained = Vec::new();
    for message in messages.iter().rev() {
        if message.role != MessageRole::User || is_compaction_summary(message) || remaining == 0 {
            continue;
        }
        let tokens = estimate_message_tokens(message).max(1);
        if tokens <= remaining {
            retained.push(message.clone());
            remaining = remaining.saturating_sub(tokens);
        } else if let Some(message) = truncate_user_message(message, remaining) {
            retained.push(message);
            remaining = 0;
        }
    }
    retained.reverse();
    retained
}

fn truncate_user_message(message: &Message, max_tokens: u64) -> Option<Message> {
    if max_tokens == 0 {
        return None;
    }
    let mut message = message.clone();
    let max_chars = max_tokens.saturating_mul(APPROX_CHARS_PER_TOKEN) as usize;
    message.content = match message.content {
        MessageContent::Text(text) => MessageContent::Text(take_last_chars(&text, max_chars)),
        MessageContent::MultiPart(parts) => {
            let mut remaining = max_chars;
            let mut kept = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Image { .. } => kept.push(part),
                    ContentPart::Text { text } if remaining > 0 => {
                        let text = if text.chars().count() <= remaining {
                            text
                        } else {
                            take_last_chars(&text, remaining)
                        };
                        remaining = remaining.saturating_sub(text.chars().count());
                        kept.push(ContentPart::Text { text });
                    }
                    _ => {}
                }
            }
            if kept.is_empty() {
                return None;
            }
            MessageContent::MultiPart(kept)
        }
    };
    Some(message)
}

fn trim_tool_outputs_to_context_window(
    input: &mut [ModelContextItem],
    instructions: &str,
    context_window: Option<u64>,
) {
    let Some(context_window) = context_window else {
        return;
    };
    for index in (0..input.len()).rev() {
        if estimate_input_tokens(instructions, input) <= context_window {
            break;
        }
        let ModelContextItem::Message { message } = &input[index] else {
            continue;
        };
        if message.role != MessageRole::Tool {
            continue;
        }
        input[index] = ModelContextItem::from(Message {
            role: MessageRole::Tool,
            content: MessageContent::Text(CONTEXT_WINDOW_TRUNCATED_OUTPUT_MESSAGE.to_string()),
            reasoning_content: None,
            metadata: message.metadata.clone(),
        });
    }
}

fn estimate_input_tokens(instructions: &str, input: &[ModelContextItem]) -> u64 {
    estimate_text_tokens(instructions)
        + input
            .iter()
            .map(|item| match item {
                ModelContextItem::Message { message } => estimate_message_tokens(message),
                ModelContextItem::Compaction { encrypted_content } => {
                    estimate_text_tokens(encrypted_content)
                }
            })
            .sum::<u64>()
}

fn estimate_text_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    chars.div_ceil(APPROX_CHARS_PER_TOKEN)
}

fn take_last_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars().rev().take(max_chars).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn user(text: &str) -> Message {
        Message {
            role: MessageRole::User,
            content: MessageContent::Text(text.to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn v2_replacement_keeps_users_and_places_checkpoint_last() {
        let replacement = build_v2_replacement(
            &[user("first"), user("second")],
            vec![ModelContextItem::Compaction {
                encrypted_content: "encrypted".to_string(),
            }],
        )
        .unwrap();

        assert_eq!(replacement.len(), 3);
        assert!(
            replacement
                .last()
                .is_some_and(ModelContextItem::is_compaction)
        );
    }

    #[test]
    fn legacy_replacement_filters_prelude_tools_and_unknown_history() {
        let original = vec![user("real user")];
        let replacement = filter_legacy_replacement(
            &original,
            vec![
                ModelContextItem::from(Message {
                    role: MessageRole::System,
                    content: MessageContent::Text("developer".to_string()),
                    reasoning_content: None,
                    metadata: HashMap::new(),
                }),
                ModelContextItem::from(user("prelude user")),
                ModelContextItem::from(user("real user")),
                ModelContextItem::from(Message {
                    role: MessageRole::Assistant,
                    content: MessageContent::Text("summary note".to_string()),
                    reasoning_content: None,
                    metadata: HashMap::new(),
                }),
                ModelContextItem::from(Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text("tool output".to_string()),
                    reasoning_content: None,
                    metadata: HashMap::new(),
                }),
                ModelContextItem::Compaction {
                    encrypted_content: "encrypted".to_string(),
                },
            ],
        )
        .unwrap();

        assert_eq!(replacement.len(), 3);
        assert!(matches!(
            &replacement[0],
            ModelContextItem::Message { message }
                if message.role == MessageRole::User
                    && message.content == MessageContent::Text("real user".to_string())
        ));
        assert!(matches!(
            &replacement[1],
            ModelContextItem::Message { message } if message.role == MessageRole::Assistant
        ));
        assert!(replacement[2].is_compaction());
    }

    #[test]
    fn tool_output_trimming_keeps_metadata_and_success_shape() {
        let metadata = HashMap::from([("tool_call_id".to_string(), "call-1".to_string())]);
        let mut input = vec![ModelContextItem::from(Message {
            role: MessageRole::Tool,
            content: MessageContent::Text("x".repeat(100)),
            reasoning_content: None,
            metadata: metadata.clone(),
        })];

        trim_tool_outputs_to_context_window(&mut input, "", Some(1));

        let ModelContextItem::Message { message } = &input[0] else {
            panic!("expected tool message");
        };
        assert_eq!(message.metadata, metadata);
        assert_eq!(
            message.content,
            MessageContent::Text(CONTEXT_WINDOW_TRUNCATED_OUTPUT_MESSAGE.to_string())
        );
    }
}

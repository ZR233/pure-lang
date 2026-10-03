//! Frozen-prefix requests for auxiliary text summarization.
use super::{CompletionRequest, Message, MessageContent, MessageRole, ModelContextItem};

/// Prepares a summary request while preserving every supplied history item.
/// Hosted tools are omitted and local tools cannot be selected. Callers provide a fresh session.
pub fn summary_request(
    instructions: &str,
    mut prefix: CompletionRequest,
    requirement: &str,
    max_output_tokens: Option<u64>,
) -> CompletionRequest {
    prefix.input.push(ModelContextItem::from(Message {
        presentation: pl_protocol::MessagePresentation::Hidden,
        role: MessageRole::User,
        content: MessageContent::text(requirement.to_owned()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    }));
    prefix.instructions = Some(instructions.to_owned());
    prefix.tools.retain(|tool| !tool.is_hosted());
    prefix.tool_choice = "none".into();
    prefix.max_tokens = max_output_tokens;
    prefix
}

//! Frozen-prefix requests for auxiliary text summarization.
use super::{CompletionRequest, Message, MessageContent, MessageRole, ModelContextItem, ToolSpec};

/// Prepares a summary request while preserving every supplied history item.
/// Hosted tools are omitted and local tools cannot be selected. Callers provide a fresh session.
pub fn summary_request(
    instructions: &str,
    mut input: Vec<ModelContextItem>,
    tools: &[ToolSpec],
    requirement: &str,
    max_output_tokens: Option<u64>,
) -> CompletionRequest {
    input.push(ModelContextItem::from(Message {
        presentation: pl_protocol::MessagePresentation::Hidden,
        role: MessageRole::User,
        content: MessageContent::text(requirement.to_owned()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    }));
    CompletionRequest::builder()
        .instructions(instructions.to_owned())
        .input(input)
        .tools(
            tools
                .iter()
                .filter(|tool| !tool.is_hosted())
                .cloned()
                .collect(),
        )
        .tool_choice("none")
        .maybe_max_tokens(max_output_tokens)
        .build()
}

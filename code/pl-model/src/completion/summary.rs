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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_preserves_history_prefix_and_disables_tool_side_effects() {
        let history = vec![ModelContextItem::from(Message {
            presentation: Default::default(),
            role: MessageRole::User,
            content: MessageContent::text("  original history\n"),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: Default::default(),
        })];
        let tools = vec![
            ToolSpec::function("local", "local", serde_json::json!({"type":"object"})),
            ToolSpec::ProgrammaticToolCalling,
        ];
        let request = summary_request(
            "frozen instructions",
            history.clone(),
            &tools,
            "summarize",
            Some(4096),
        );
        assert_eq!(request.input[..history.len()], history);
        assert_eq!(request.tools, vec![tools[0].clone()]);
        assert_eq!(request.tool_choice, "none");
        assert_eq!(request.input.len(), history.len() + 1);
    }
}

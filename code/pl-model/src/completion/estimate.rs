//! Explicitly approximate text-only estimation, independent of service-reported usage.
use super::{CompletionRequest, ContentPart, ModelContextItem};

/// Estimates text input using four Unicode scalar values per token.
///
/// This is a heuristic, not a provider tokenizer or a capacity guarantee. Native opaque
/// records and resources return `None`; their encoded string length does not identify
/// their model-visible token cost. Overflow and encoding failure also remain unknown.
pub fn estimate_text_input_tokens(request: &CompletionRequest) -> Option<u64> {
    if !request.prepared_content.is_empty() {
        return None;
    }
    let mut characters = request
        .instructions
        .as_deref()
        .map_or(0, |text| text.chars().count());
    for item in &request.input {
        let message = match item {
            ModelContextItem::Message { message }
            | ModelContextItem::ToolResult { message, .. } => message,
            ModelContextItem::Compaction { .. }
            | ModelContextItem::Responses { .. }
            | ModelContextItem::ToolMedia { .. } => return None,
        };
        for part in &message.content.parts {
            match part {
                ContentPart::Text { text } => {
                    characters = characters.checked_add(text.chars().count())?
                }
                ContentPart::Attachment { .. } => return None,
            }
        }
        if let Some(reasoning) = &message.reasoning_content {
            characters = characters.checked_add(reasoning.chars().count())?;
        }
        if let Some(calls) = &message.tool_calls {
            characters =
                characters.checked_add(serde_json::to_string(calls).ok()?.chars().count())?;
        }
    }
    for tool in &request.tools {
        characters = characters.checked_add(serde_json::to_string(tool).ok()?.chars().count())?;
    }
    u64::try_from(characters)
        .ok()
        .map(|characters| characters.div_ceil(4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_material_does_not_turn_into_a_ciphertext_length_estimate() {
        let request = CompletionRequest::builder()
            .input(vec![ModelContextItem::Compaction {
                encrypted_content: "opaque".repeat(1000),
            }])
            .build();
        assert_eq!(estimate_text_input_tokens(&request), None);
    }

    #[test]
    fn plain_text_estimate_remains_explicitly_approximate() {
        let request = CompletionRequest::builder()
            .instructions("abcdefgh")
            .build();
        assert_eq!(estimate_text_input_tokens(&request), Some(2));
    }
}

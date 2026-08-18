use std::collections::HashMap;

use pl_model::{CompletionRequest, ModelProvider, ReasoningConfig, TokenUsage};
use pl_protocol::{Message, MessageContent, MessageRole, ModelContextItem, PureError, Result};
use pl_trace::AgentEventSender;

use super::ContextCompactionConfig;
use super::history::build_compacted_history;
use crate::TraceRecorder;
use crate::core::progress::ProgressEmitter;

#[allow(clippy::too_many_arguments)]
pub(super) async fn compact_local(
    provider: &(impl ModelProvider + ?Sized),
    model: &str,
    config: &ContextCompactionConfig,
    request_instructions: &str,
    request_messages: &[Message],
    session_items: &[ModelContextItem],
    working_context_tail: Option<&Message>,
    event_tx: AgentEventSender,
    recorder: &mut TraceRecorder,
    progress: &mut Option<&mut ProgressEmitter>,
    max_output_tokens: Option<u64>,
) -> Result<(Vec<ModelContextItem>, TokenUsage, String)> {
    let mut input = request_messages
        .iter()
        .cloned()
        .map(ModelContextItem::from)
        .chain(session_items.iter().cloned())
        .collect::<Vec<_>>();
    if let Some(tail) = working_context_tail {
        input.push(ModelContextItem::from(tail.clone()));
    }
    super::compact_old_tool_results_for_request(&mut input);
    input.push(ModelContextItem::from(Message {
        role: MessageRole::User,
        content: MessageContent::Text(config.instructions.clone()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }));
    let mut max_tokens = Some(max_output_tokens.unwrap_or(4096).min(4096));
    loop {
        let completion_request = CompletionRequest::builder(model)
            .instructions(request_instructions.to_string())
            .input(input.clone())
            .tool_choice("none")
            .maybe_max_tokens(max_tokens)
            .store(Some(false))
            .reasoning(None::<ReasoningConfig>)
            .build();
        let response = match provider
            .stream_complete(completion_request, event_tx.clone())
            .await
        {
            Ok(response) => response,
            Err(error) if max_tokens.is_some() && is_unsupported_max_output_tokens(&error) => {
                max_tokens = None;
                if let Some(progress) = progress.as_deref_mut() {
                    progress.milestone(
                        recorder,
                        "模型不支持压缩请求的 max_output_tokens 参数，正在不带该参数重试。",
                    );
                }
                continue;
            }
            Err(error) if can_retry_context_items(&error, &mut input) => {
                if let Some(progress) = progress.as_deref_mut() {
                    progress.milestone(recorder, "上下文压缩请求过大，正在缩小历史后重试。");
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        let Some(summary) = response
            .content
            .filter(|content| !content.trim().is_empty())
        else {
            return Err(PureError::LlmError(config.empty_summary_error.clone()));
        };
        let original_messages = session_items
            .iter()
            .filter_map(ModelContextItem::as_message)
            .cloned()
            .collect::<Vec<_>>();
        let replacement = build_compacted_history(&original_messages, &summary, config)
            .into_iter()
            .map(ModelContextItem::from)
            .collect();
        return Ok((replacement, response.usage, summary));
    }
}

fn can_retry_context_items(error: &PureError, input: &mut Vec<ModelContextItem>) -> bool {
    if !is_context_pressure_error(error) || input.len() <= 1 {
        return false;
    }
    input.remove(0);
    while input
        .first()
        .and_then(ModelContextItem::as_message)
        .is_some_and(|message| message.role == MessageRole::Tool)
    {
        input.remove(0);
    }
    true
}

fn is_unsupported_max_output_tokens(error: &PureError) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("max_output_tokens")
        && (text.contains("unsupported parameter")
            || text.contains("unknown parameter")
            || text.contains("unrecognized parameter"))
}

fn is_context_pressure_error(error: &PureError) -> bool {
    match error {
        PureError::ContextOverflow(_) => true,
        other => {
            let text = other.to_string().to_ascii_lowercase();
            text.contains("context")
                || text.contains("maximum")
                || text.contains("too many tokens")
                || text.contains("token limit")
        }
    }
}

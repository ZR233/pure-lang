use std::collections::HashMap;

use pl_model::{CompletionRequest, ModelProvider, ReasoningConfig, TokenUsage};
use pl_protocol::{ContentPart, Message, MessageContent, MessageRole, PureError, Result};
use pl_trace::AgentEventSender;

use crate::core::progress::ProgressEmitter;
use crate::session::CoreSession;

const COMPACT_PROMPT: &str = include_str!("../prompts/compact.md");
const RECENT_USER_TOKEN_BUDGET: u64 = 20_000;
const APPROX_CHARS_PER_TOKEN: u64 = 4;
pub(crate) const SUMMARY_METADATA_KEY: &str = "context_compaction";
pub(crate) const SUMMARY_METADATA_VALUE: &str = "summary";
const SUMMARY_PREFIX: &str = "以下是此前对话的压缩摘要。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionTrigger {
    EstimatedTokens,
    ProviderPromptTokens(u64),
}

#[derive(Debug, Clone)]
pub(crate) enum CompactionOutcome {
    Skipped,
    Compacted { usage: TokenUsage },
}

pub(crate) struct ContextCompactionRequest<'a, P: ModelProvider + ?Sized> {
    pub provider: &'a P,
    pub model: &'a str,
    pub request_instructions: &'a str,
    pub request_messages: &'a [Message],
    pub trigger: CompactionTrigger,
    pub event_tx: AgentEventSender,
    pub progress: Option<&'a mut ProgressEmitter>,
}

pub(crate) async fn maybe_compact_session(
    session: &mut CoreSession,
    request: ContextCompactionRequest<'_, impl ModelProvider + ?Sized>,
) -> Result<CompactionOutcome> {
    let ContextCompactionRequest {
        provider,
        model,
        request_instructions,
        request_messages,
        trigger,
        event_tx,
        mut progress,
    } = request;
    let model_info = provider.model_info(model);
    let Some(limit) = model_info.resolved_auto_compact_limit() else {
        return Ok(CompactionOutcome::Skipped);
    };
    if !has_compactable_history(session.messages()) {
        return Ok(CompactionOutcome::Skipped);
    }
    let estimated_tokens =
        estimate_request_tokens(request_instructions, request_messages, session.messages());
    let should_compact = match trigger {
        CompactionTrigger::EstimatedTokens => estimated_tokens >= limit,
        CompactionTrigger::ProviderPromptTokens(prompt_tokens) => {
            prompt_tokens >= limit || estimated_tokens >= limit
        }
    };
    if !should_compact {
        return Ok(CompactionOutcome::Skipped);
    }

    if let Some(progress) = progress.as_deref_mut() {
        progress.milestone("上下文接近上限，正在压缩历史。");
    }
    let mut messages = session.messages().to_vec();
    let max_tokens = Some(model_info.max_output_tokens.unwrap_or(4096).min(4096));
    loop {
        let completion_request = CompletionRequest {
            model: model.to_string(),
            instructions: Some(COMPACT_PROMPT.to_string()),
            messages: compaction_prompt_messages(&messages),
            tools: Vec::new(),
            tool_choice: "none".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens,
            store: None,
            previous_response_id: None,
            prompt_cache_key: None,
            reasoning: None::<ReasoningConfig>,
            stream: true,
            trace: None,
        };
        let response = match provider
            .stream_complete(completion_request, event_tx.clone())
            .await
        {
            Ok(response) => response,
            Err(error) if can_retry_compaction(&error, &mut messages) => {
                if let Some(progress) = progress.as_deref_mut() {
                    progress.milestone("上下文压缩请求过大，正在缩小历史后重试。");
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        let Some(summary) = response
            .content
            .filter(|content| !content.trim().is_empty())
        else {
            return Err(PureError::LlmError(
                "context compaction returned an empty summary".to_string(),
            ));
        };
        let replacement = build_compacted_history(&messages, &summary);
        session.replace_messages(replacement);
        if let Some(progress) = progress.as_deref_mut() {
            progress.milestone("上下文已压缩，继续准备模型调用。");
        }
        return Ok(CompactionOutcome::Compacted {
            usage: response.usage,
        });
    }
}

fn compaction_prompt_messages(messages: &[Message]) -> Vec<Message> {
    let mut prompt_messages = messages.to_vec();
    prompt_messages.push(Message {
        role: MessageRole::User,
        content: MessageContent::Text("请根据以上完整上下文生成压缩摘要。".to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    });
    prompt_messages
}

fn can_retry_compaction(error: &PureError, messages: &mut Vec<Message>) -> bool {
    if !is_context_pressure_error(error) {
        return false;
    }
    remove_oldest_retriable_message(messages)
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

fn remove_oldest_retriable_message(messages: &mut Vec<Message>) -> bool {
    if messages.len() <= 1 {
        return false;
    }
    let first_has_tool_calls = messages
        .first()
        .is_some_and(|message| message.metadata.contains_key("tool_calls"));
    messages.remove(0);
    if first_has_tool_calls {
        while matches!(
            messages.first().map(|message| message.role),
            Some(MessageRole::Tool)
        ) {
            messages.remove(0);
        }
    }
    while matches!(
        messages.first().map(|message| message.role),
        Some(MessageRole::Tool)
    ) {
        messages.remove(0);
    }
    true
}

fn has_compactable_history(messages: &[Message]) -> bool {
    let raw_messages = messages
        .iter()
        .filter(|message| !is_compaction_summary(message))
        .count();
    raw_messages > 1
}

pub(crate) fn build_compacted_history(messages: &[Message], summary: &str) -> Vec<Message> {
    let mut compacted = Vec::new();
    compacted.push(summary_message(summary));
    compacted.extend(recent_user_messages(messages, RECENT_USER_TOKEN_BUDGET));
    compacted
}

fn summary_message(summary: &str) -> Message {
    let mut metadata = HashMap::new();
    metadata.insert(
        SUMMARY_METADATA_KEY.to_string(),
        SUMMARY_METADATA_VALUE.to_string(),
    );
    let trimmed = summary.trim();
    Message {
        role: MessageRole::User,
        content: MessageContent::Text(format!("{SUMMARY_PREFIX}\n\n{trimmed}")),
        reasoning_content: None,
        metadata,
    }
}

fn recent_user_messages(messages: &[Message], token_budget: u64) -> Vec<Message> {
    let mut selected = Vec::new();
    let mut used_tokens = 0_u64;
    let mut first_user = true;
    for message in messages.iter().rev() {
        if message.role != MessageRole::User || is_compaction_summary(message) {
            continue;
        }
        let message_tokens = estimate_message_tokens(message);
        if !first_user && used_tokens.saturating_add(message_tokens) > token_budget {
            continue;
        }
        selected.push(message.clone());
        used_tokens = used_tokens.saturating_add(message_tokens);
        first_user = false;
    }
    selected.reverse();
    selected
}

fn is_compaction_summary(message: &Message) -> bool {
    message
        .metadata
        .get(SUMMARY_METADATA_KEY)
        .is_some_and(|value| value == SUMMARY_METADATA_VALUE)
}

fn estimate_request_tokens(
    instructions: &str,
    request_messages: &[Message],
    session_messages: &[Message],
) -> u64 {
    estimate_text_tokens(instructions)
        + request_messages
            .iter()
            .chain(session_messages)
            .map(estimate_message_tokens)
            .sum::<u64>()
}

fn estimate_message_tokens(message: &Message) -> u64 {
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

fn estimate_text_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    if chars == 0 {
        0
    } else {
        chars.div_ceil(APPROX_CHARS_PER_TOKEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::progress::{ProgressEmitter, ProgressVerbosity};
    use pl_model::{
        CompletionEventStream, CompletionResponse, FinishReason, ModelCapabilities, ModelInfo,
        ProviderCapabilities, ProviderInfo,
    };
    use pl_trace::{AgentEvent, TracePartSource};
    use pretty_assertions::assert_eq;
    use std::sync::{Arc, Mutex};

    fn text_message(role: MessageRole, text: &str) -> Message {
        Message {
            role,
            content: MessageContent::Text(text.to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn compacted_history_filters_old_summary_and_keeps_latest_user() {
        let mut old_summary = text_message(MessageRole::User, "old summary");
        old_summary.metadata.insert(
            SUMMARY_METADATA_KEY.to_string(),
            SUMMARY_METADATA_VALUE.to_string(),
        );
        let messages = vec![
            old_summary,
            text_message(MessageRole::User, "old request"),
            text_message(MessageRole::Assistant, "old answer"),
            text_message(MessageRole::Tool, "tool output"),
            text_message(MessageRole::User, "latest request"),
        ];

        let compacted = build_compacted_history(&messages, "new summary");

        assert_eq!(compacted.len(), 3);
        assert!(is_compaction_summary(&compacted[0]));
        assert_eq!(message_text(&compacted[1]), "old request");
        assert_eq!(message_text(&compacted[2]), "latest request");
    }

    #[test]
    fn compacted_history_keeps_latest_user_even_when_budget_is_small() {
        let messages = vec![
            text_message(MessageRole::User, "first request"),
            text_message(MessageRole::User, "latest request with a long body"),
        ];

        let users = recent_user_messages(&messages, 1);

        assert_eq!(users.len(), 1);
        assert_eq!(message_text(&users[0]), "latest request with a long body");
    }

    #[test]
    fn compactable_history_requires_more_than_current_prompt() {
        let messages = vec![text_message(MessageRole::User, "latest request")];

        assert!(!has_compactable_history(&messages));
    }

    #[tokio::test]
    async fn context_compaction_retry_emits_runtime_progress() {
        let mut model = ModelInfo::fallback("compact-test");
        model.context_window = Some(100);
        model.max_context_window = Some(100);
        model.auto_compact_token_limit = Some(1);
        let provider = FakeCompactionProvider::new(model);
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut progress =
            ProgressEmitter::new(event_tx.clone(), "turn-compact", ProgressVerbosity::Normal);
        let mut session = CoreSession::new();
        session.push_user_prompt("old request ".repeat(20));
        session.push_assistant_response("old answer ".repeat(20), None);
        session.push_user_prompt("latest request ".repeat(20));

        let outcome = maybe_compact_session(
            &mut session,
            ContextCompactionRequest {
                provider: &provider,
                model: "compact-test",
                request_instructions: "",
                request_messages: &[],
                trigger: CompactionTrigger::EstimatedTokens,
                event_tx,
                progress: Some(&mut progress),
            },
        )
        .await
        .unwrap();

        assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
        let recorded_messages = provider.recorded_message_counts();
        assert_eq!(recorded_messages, vec![4, 3]);
        assert_eq!(
            runtime_progress_texts(&mut event_rx),
            vec![
                "上下文接近上限，正在压缩历史。".to_string(),
                "上下文压缩请求过大，正在缩小历史后重试。".to_string(),
                "上下文已压缩，继续准备模型调用。".to_string(),
            ]
        );
    }

    fn message_text(message: &Message) -> &str {
        match &message.content {
            MessageContent::Text(text) => text,
            MessageContent::MultiPart(_) => panic!("expected text message"),
        }
    }

    fn runtime_progress_texts(
        event_rx: &mut tokio::sync::broadcast::Receiver<AgentEvent>,
    ) -> Vec<String> {
        let mut texts = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            if let AgentEvent::TracePartCompleted { item } = event
                && item.source == TracePartSource::Runtime
            {
                texts.push(item.content);
            }
        }
        texts
    }

    #[derive(Debug)]
    struct FakeCompactionProvider {
        info: ProviderInfo,
        model: ModelInfo,
        calls: Arc<Mutex<Vec<CompletionRequest>>>,
    }

    impl FakeCompactionProvider {
        fn new(model: ModelInfo) -> Self {
            let mut info = ProviderInfo::openai(Some("http://example.invalid".to_string()));
            info.default_model = model.slug.clone();
            Self {
                info,
                model,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn recorded_message_counts(&self) -> Vec<usize> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.messages.len())
                .collect()
        }
    }

    impl ModelProvider for FakeCompactionProvider {
        fn info(&self) -> &ProviderInfo {
            &self.info
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::STREAMING
        }

        async fn stream_events(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionEventStream> {
            Err(PureError::LlmError(
                "fake compaction provider does not stream events".to_string(),
            ))
        }

        async fn stream_complete(
            &self,
            request: CompletionRequest,
            _event_tx: AgentEventSender,
        ) -> Result<CompletionResponse> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(request);
            if calls.len() == 1 {
                return Err(PureError::LlmError(
                    "context token limit exceeded".to_string(),
                ));
            }
            Ok(CompletionResponse {
                response_id: None,
                content: Some("summary".to_string()),
                raw_content: Some("summary".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                trace_events: Vec::new(),
                next_sequence: 0,
                usage: TokenUsage {
                    prompt_tokens: 1,
                    completion_tokens: 2,
                    total_tokens: 3,
                    cached_prompt_tokens: 0,
                    reasoning_tokens: 0,
                },
                finish_reason: FinishReason::Stop,
                model: self.model.slug.clone(),
            })
        }

        async fn auth_token(&self) -> Result<Option<String>> {
            Ok(None)
        }

        fn model_info(&self, model: &str) -> ModelInfo {
            if model == self.model.slug {
                self.model.clone()
            } else {
                ModelInfo::fallback(model)
            }
        }

        fn list_models(&self) -> Vec<ModelInfo> {
            vec![self.model.clone()]
        }

        fn effective_model_capabilities(&self, model: &str) -> ModelCapabilities {
            self.model_info(model).capabilities
        }

        fn default_model(&self) -> &str {
            &self.info.default_model
        }
    }
}

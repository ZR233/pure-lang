use std::collections::HashMap;

use pl_model::{CompletionRequest, ModelProvider, ReasoningConfig, TokenUsage};
use pl_protocol::{
    ContentPart, Message, MessageContent, MessageRole, PureError, Result,
    TOOL_CALL_CALL_ID_METADATA_KEY, TOOL_CALL_ID_METADATA_KEY,
};
use pl_trace::AgentEventSender;

use crate::core::progress::ProgressEmitter;
use crate::session::CoreSession;

const DEFAULT_COMPACT_PROMPT: &str = include_str!("../prompts/compact.md");
const DEFAULT_SUMMARY_REQUEST: &str = "请根据以上完整上下文生成压缩摘要。";
const RECENT_USER_TOKEN_BUDGET: u64 = 20_000;
const APPROX_CHARS_PER_TOKEN: u64 = 4;
pub(crate) const SUMMARY_METADATA_KEY: &str = "context_compaction";
pub(crate) const SUMMARY_METADATA_VALUE: &str = "summary";
const DEFAULT_SUMMARY_PREFIX: &str = "以下是此前对话的压缩摘要。";
const DEFAULT_EMPTY_SUMMARY_ERROR: &str = "context compaction returned an empty summary";

/// 上下文压缩的产品级文案配置。
///
/// `pl-core` 提供默认提示词；宿主可以替换 prompt、摘要前缀和错误文案，
/// 以保持已有历史回放或 UI 识别语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionConfig {
    pub instructions: String,
    pub summary_request: String,
    pub summary_prefix: String,
    pub empty_summary_error: String,
    pub replacement: ContextCompactionReplacement,
}

impl ContextCompactionConfig {
    pub fn new(
        instructions: impl Into<String>,
        summary_request: impl Into<String>,
        summary_prefix: impl Into<String>,
        empty_summary_error: impl Into<String>,
    ) -> Self {
        Self {
            instructions: instructions.into(),
            summary_request: summary_request.into(),
            summary_prefix: summary_prefix.into(),
            empty_summary_error: empty_summary_error.into(),
            replacement: ContextCompactionReplacement::default(),
        }
    }

    pub fn with_replacement(mut self, replacement: ContextCompactionReplacement) -> Self {
        self.replacement = replacement;
        self
    }
}

impl Default for ContextCompactionConfig {
    fn default() -> Self {
        Self {
            instructions: DEFAULT_COMPACT_PROMPT.to_string(),
            summary_request: DEFAULT_SUMMARY_REQUEST.to_string(),
            summary_prefix: DEFAULT_SUMMARY_PREFIX.to_string(),
            empty_summary_error: DEFAULT_EMPTY_SUMMARY_ERROR.to_string(),
            replacement: ContextCompactionReplacement::default(),
        }
    }
}

/// 压缩后历史的保留策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextCompactionReplacement {
    SummaryThenRecentUsers { token_budget: u64 },
    RecentInteractionTail(RecentInteractionTailConfig),
}

impl Default for ContextCompactionReplacement {
    fn default() -> Self {
        Self::SummaryThenRecentUsers {
            token_budget: RECENT_USER_TOKEN_BUDGET,
        }
    }
}

/// 压缩后保留近期交互尾部的限制。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentInteractionTailConfig {
    pub max_user_chars: usize,
    pub max_assistant_chars: usize,
    pub max_tool_output_chars: usize,
    pub assistant_items: usize,
    pub tool_output_items: usize,
}

/// 触发上下文压缩的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionTrigger {
    EstimatedTokens,
    ProviderPromptTokens,
}

impl ContextCompactionTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EstimatedTokens => "estimatedTokens",
            Self::ProviderPromptTokens => "providerPromptTokens",
        }
    }
}

/// 单次上下文压缩的可观测快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionSnapshot {
    pub trigger: ContextCompactionTrigger,
    pub tokens_before: u64,
    pub estimated_request_tokens: u64,
    pub provider_prompt_tokens: Option<u64>,
    pub auto_compact_limit: u64,
    pub replacement_tokens: Option<u64>,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionTrigger {
    EstimatedTokens,
    ProviderPromptTokens(u64),
}

#[derive(Debug, Clone)]
pub(crate) enum CompactionOutcome {
    Skipped,
    Compacted {
        usage: TokenUsage,
        snapshot: ContextCompactionSnapshot,
    },
}

pub(crate) struct ContextCompactionRequest<'a, P: ModelProvider + ?Sized> {
    pub provider: &'a P,
    pub model: &'a str,
    pub config: &'a ContextCompactionConfig,
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
        config,
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
    let mut max_tokens = Some(model_info.max_output_tokens.unwrap_or(4096).min(4096));
    loop {
        let completion_request = CompletionRequest::builder(model)
            .instructions(config.instructions.clone())
            .messages(compaction_prompt_messages(
                &messages,
                &config.summary_request,
            ))
            .tool_choice("none")
            .maybe_max_tokens(max_tokens)
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
                        "模型不支持压缩请求的 max_output_tokens 参数，正在不带该参数重试。",
                    );
                }
                continue;
            }
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
            return Err(PureError::LlmError(config.empty_summary_error.clone()));
        };
        let replacement = build_compacted_history(&messages, &summary, config);
        let replacement_tokens = Some(estimate_request_tokens(
            request_instructions,
            request_messages,
            &replacement,
        ));
        let snapshot = ContextCompactionSnapshot {
            trigger: public_trigger(trigger),
            tokens_before: tokens_before(trigger, estimated_tokens),
            estimated_request_tokens: estimated_tokens,
            provider_prompt_tokens: provider_prompt_tokens(trigger),
            auto_compact_limit: limit,
            replacement_tokens,
            summary: summary.clone(),
        };
        session.replace_messages(replacement);
        if let Some(progress) = progress.as_deref_mut() {
            progress.milestone("上下文已压缩，继续准备模型调用。");
        }
        return Ok(CompactionOutcome::Compacted {
            usage: response.usage,
            snapshot,
        });
    }
}

fn public_trigger(trigger: CompactionTrigger) -> ContextCompactionTrigger {
    match trigger {
        CompactionTrigger::EstimatedTokens => ContextCompactionTrigger::EstimatedTokens,
        CompactionTrigger::ProviderPromptTokens(_) => {
            ContextCompactionTrigger::ProviderPromptTokens
        }
    }
}

fn provider_prompt_tokens(trigger: CompactionTrigger) -> Option<u64> {
    match trigger {
        CompactionTrigger::EstimatedTokens => None,
        CompactionTrigger::ProviderPromptTokens(tokens) => Some(tokens),
    }
}

fn tokens_before(trigger: CompactionTrigger, estimated_tokens: u64) -> u64 {
    provider_prompt_tokens(trigger).unwrap_or(estimated_tokens)
}

fn compaction_prompt_messages(messages: &[Message], summary_request: &str) -> Vec<Message> {
    let mut prompt_messages = messages.to_vec();
    prompt_messages.push(Message {
        role: MessageRole::User,
        content: MessageContent::Text(summary_request.to_string()),
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

fn message_text(message: &Message) -> String {
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

fn is_compaction_summary(message: &Message) -> bool {
    message
        .metadata
        .get(SUMMARY_METADATA_KEY)
        .is_some_and(|value| value == SUMMARY_METADATA_VALUE)
}

fn is_summary_text(text: &str, summary_prefix: &str) -> bool {
    text.starts_with(summary_prefix)
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

        let config = ContextCompactionConfig::default();
        let compacted = build_compacted_history(&messages, "new summary", &config);

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
                config: &ContextCompactionConfig::default(),
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

    #[tokio::test]
    async fn context_compaction_retries_without_max_output_tokens_when_provider_rejects_parameter()
    {
        let mut model = ModelInfo::fallback("compact-test");
        model.context_window = Some(100);
        model.max_context_window = Some(100);
        model.auto_compact_token_limit = Some(1);
        model.max_output_tokens = Some(4096);
        let provider = FakeCompactionProvider::new(model)
            .with_first_failure(FakeCompactionFailure::UnsupportedMaxOutputTokens);
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let mut session = CoreSession::new();
        session.push_user_prompt("old request ".repeat(20));
        session.push_assistant_response("old answer ".repeat(20), None);
        session.push_user_prompt("latest request ".repeat(20));

        let outcome = maybe_compact_session(
            &mut session,
            ContextCompactionRequest {
                provider: &provider,
                model: "compact-test",
                config: &ContextCompactionConfig::default(),
                request_instructions: "",
                request_messages: &[],
                trigger: CompactionTrigger::EstimatedTokens,
                event_tx,
                progress: None,
            },
        )
        .await
        .unwrap();

        assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
        assert_eq!(provider.recorded_message_counts(), vec![4, 4]);
        assert_eq!(provider.recorded_max_tokens(), vec![Some(4096), None]);
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
        first_failure: FakeCompactionFailure,
    }

    impl FakeCompactionProvider {
        fn new(model: ModelInfo) -> Self {
            let mut info = ProviderInfo::openai(Some("http://example.invalid".to_string()));
            info.default_model = model.slug.clone();
            Self {
                info,
                model,
                calls: Arc::new(Mutex::new(Vec::new())),
                first_failure: FakeCompactionFailure::ContextPressure,
            }
        }

        fn with_first_failure(mut self, first_failure: FakeCompactionFailure) -> Self {
            self.first_failure = first_failure;
            self
        }

        fn recorded_message_counts(&self) -> Vec<usize> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.messages.len())
                .collect()
        }

        fn recorded_max_tokens(&self) -> Vec<Option<u64>> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.max_tokens)
                .collect()
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum FakeCompactionFailure {
        ContextPressure,
        UnsupportedMaxOutputTokens,
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
                return match self.first_failure {
                    FakeCompactionFailure::ContextPressure => Err(PureError::LlmError(
                        "context token limit exceeded".to_string(),
                    )),
                    FakeCompactionFailure::UnsupportedMaxOutputTokens => Err(
                        PureError::LlmError(
                            "HTTP error: missing field `error` at line 1 column 53: {\"detail\":\"Unsupported parameter: max_output_tokens\"}"
                                .to_string(),
                        ),
                    ),
                };
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

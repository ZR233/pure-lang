use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pl_model::{
    CompletionEventStream, CompletionRequest, CompletionResponse, FinishReason, ModelCapabilities,
    ModelInfo, ModelTransportProfile, OpenAiCompactionMode, ProviderCapabilities, ProviderInfo,
    ProviderWireProtocol, TokenUsage,
};
use pl_protocol::{Message, MessageContent, MessageRole, ModelContextItem, PureError, Result};
use pl_trace::{AgentEvent, AgentEventSender, TracePartSource};
use pretty_assertions::assert_eq;

use super::history::{
    has_compactable_history, is_compaction_summary, message_text, recent_user_messages,
};
use super::*;
use crate::core::progress::{ProgressEmitter, ProgressVerbosity};

fn text_message(role: MessageRole, text: &str) -> Message {
    Message {
        role,
        content: MessageContent::Text(text.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

#[test]
fn compacted_history_filters_old_summary_and_places_new_summary_last() {
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

    let compacted = build_compacted_history(
        &messages,
        "new summary",
        &ContextCompactionConfig::default(),
    );

    assert_eq!(compacted.len(), 3);
    assert_eq!(message_text(&compacted[0]), "old request");
    assert_eq!(message_text(&compacted[1]), "latest request");
    assert!(is_compaction_summary(&compacted[2]));
}

#[test]
fn recent_user_boundary_is_truncated_to_token_budget() {
    let messages = vec![
        text_message(MessageRole::User, "first request"),
        text_message(MessageRole::User, "latest request with a long body"),
    ];

    let users = recent_user_messages(&messages, 1);

    assert_eq!(users.len(), 1);
    assert_eq!(message_text(&users[0]), "body");
}

#[test]
fn manual_compaction_accepts_one_real_message_but_auto_does_not() {
    let items = vec![ModelContextItem::from(text_message(
        MessageRole::User,
        "latest request",
    ))];

    assert!(!has_compactable_history(
        &items,
        CompactionTrigger::EstimatedTokens
    ));
    assert!(has_compactable_history(&items, CompactionTrigger::Manual));
    assert!(!has_compactable_history(
        &[ModelContextItem::Compaction {
            encrypted_content: "encrypted".to_string(),
        }],
        CompactionTrigger::Manual,
    ));
}

#[tokio::test]
async fn local_context_pressure_retry_emits_progress_and_preserves_summary_order() {
    let provider =
        FakeCompactionProvider::new(test_model(), FakeCompactionFailure::ContextPressure);
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let mut progress = ProgressEmitter::new("turn-compact", ProgressVerbosity::Normal);
    let mut session = test_session();
    let config = ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

    let outcome = maybe_compact_session(
        &mut session,
        compaction_request(
            &provider,
            &config,
            event_tx,
            &mut recorder,
            Some(&mut progress),
        ),
    )
    .await
    .unwrap();

    assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
    assert_eq!(provider.recorded_input_counts(), vec![4, 3]);
    assert!(session.messages().last().is_some_and(is_compaction_summary));
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
async fn local_retries_without_unsupported_max_output_tokens() {
    let provider = FakeCompactionProvider::new(
        test_model(),
        FakeCompactionFailure::UnsupportedMaxOutputTokens,
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let mut session = test_session();
    let config = ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

    maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap();

    assert_eq!(provider.recorded_input_counts(), vec![4, 4]);
    assert_eq!(provider.recorded_max_tokens(), vec![Some(4096), None]);
}

#[tokio::test]
async fn local_empty_summary_preserves_session_history_and_revision() {
    let provider = FakeCompactionProvider::new(test_model(), FakeCompactionFailure::EmptySummary);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let mut session = test_session();
    let original_items = session.items().to_vec();
    let original_revision = session.revision();
    let config = ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

    let error = maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains(&config.empty_summary_error));
    assert_eq!(session.items(), original_items.as_slice());
    assert_eq!(session.revision(), original_revision);
}

#[tokio::test]
async fn remote_failure_does_not_replace_session_history_or_revision() {
    let provider = FakeCompactionProvider::new(
        responses_test_model(),
        FakeCompactionFailure::ContextPressure,
    );
    let mut session = test_session();
    let original_items = session.items().to_vec();
    let original_revision = session.revision();
    let config =
        ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());

    let error = maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("does not support remote context compaction")
    );
    assert_eq!(session.items(), original_items.as_slice());
    assert_eq!(session.revision(), original_revision);
}

#[tokio::test]
async fn chat_completions_provider_always_uses_local_compaction() {
    let mut provider =
        FakeCompactionProvider::new(test_model(), FakeCompactionFailure::ContextPressure);
    provider.info.protocol = ProviderWireProtocol::ChatCompletions;
    let mut session = test_session();
    let config =
        ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());

    let outcome = maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap();

    let CompactionOutcome::Compacted { snapshot, .. } = outcome else {
        panic!("expected compaction");
    };
    assert_eq!(
        snapshot.implementation,
        ContextCompactionImplementation::Local
    );
    assert_eq!(provider.recorded_input_counts(), vec![4, 3]);
}

#[test]
fn encrypted_checkpoint_rejects_chat_completions_provider() {
    let session = AgentSession::from_items(vec![ModelContextItem::Compaction {
        encrypted_content: "encrypted".to_string(),
    }]);

    let error =
        ensure_provider_can_consume_session(ProviderWireProtocol::ChatCompletions, &session)
            .unwrap_err();

    assert!(error.to_string().contains("继续使用 Responses provider"));
}

#[test]
fn responses_native_context_rejects_chat_completions_provider() {
    let session = AgentSession::from_items(vec![ModelContextItem::Responses {
        item: pl_protocol::ResponsesContextItem {
            kind: pl_protocol::ResponsesContextItemKind::Program,
            value: serde_json::json!({"type": "program", "id": "program-1"}),
        },
    }]);

    let error =
        ensure_provider_can_consume_session(ProviderWireProtocol::ChatCompletions, &session)
            .unwrap_err();

    assert!(error.to_string().contains("Responses provider"));
}

fn test_model() -> ModelInfo {
    let mut model = ModelInfo::fallback("compact-test");
    model.context_window = Some(100);
    model.max_context_window = Some(100);
    model.auto_compact_token_limit = Some(1);
    model.max_output_tokens = Some(4096);
    model
}

fn responses_test_model() -> ModelInfo {
    let mut model = test_model();
    model.transport = ModelTransportProfile::responses_http();
    model
}

fn test_session() -> AgentSession {
    let mut session = AgentSession::new();
    session.push_user_prompt("old request ".repeat(20));
    session.push_assistant_response("old answer ".repeat(20), None);
    session.push_user_prompt("latest request ".repeat(20));
    session
}

fn compaction_request<'a>(
    provider: &'a FakeCompactionProvider,
    config: &'a ContextCompactionConfig,
    event_tx: AgentEventSender,
    recorder: &'a mut TraceRecorder,
    progress: Option<&'a mut ProgressEmitter>,
) -> ContextCompactionRequest<'a, FakeCompactionProvider> {
    ContextCompactionRequest {
        provider,
        model: "compact-test",
        config,
        request_instructions: "",
        request_messages: &[],
        working_context_tail: None,
        tools: &[],
        parallel_tool_calls: false,
        reasoning: None,
        prompt_cache_key: None,
        trigger: CompactionTrigger::EstimatedTokens,
        phase: ContextCompactionPhase::PreTurn,
        event_tx,
        recorder,
        progress,
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

#[derive(Debug, Clone, Copy)]
enum FakeCompactionFailure {
    ContextPressure,
    UnsupportedMaxOutputTokens,
    EmptySummary,
}

#[derive(Debug)]
struct FakeCompactionProvider {
    info: ProviderInfo,
    model: ModelInfo,
    calls: Arc<Mutex<Vec<CompletionRequest>>>,
    first_failure: FakeCompactionFailure,
}

impl FakeCompactionProvider {
    fn new(model: ModelInfo, first_failure: FakeCompactionFailure) -> Self {
        let mut info = ProviderInfo::openai(Some("http://example.invalid".to_string()));
        info.default_model = model.slug.clone();
        Self {
            info,
            model,
            calls: Arc::new(Mutex::new(Vec::new())),
            first_failure,
        }
    }

    fn recorded_input_counts(&self) -> Vec<usize> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.input.len())
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

impl ModelProvider for FakeCompactionProvider {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::STREAMING
    }

    async fn stream_events(&self, _request: CompletionRequest) -> Result<CompletionEventStream> {
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
            match self.first_failure {
                FakeCompactionFailure::ContextPressure => {
                    return Err(PureError::LlmError(
                        "context token limit exceeded".to_string(),
                    ));
                }
                FakeCompactionFailure::UnsupportedMaxOutputTokens => {
                    return Err(PureError::LlmError(
                        "Unsupported parameter: max_output_tokens".to_string(),
                    ));
                }
                FakeCompactionFailure::EmptySummary => {}
            }
        }
        let content = match self.first_failure {
            FakeCompactionFailure::EmptySummary => Some("   ".to_string()),
            FakeCompactionFailure::ContextPressure
            | FakeCompactionFailure::UnsupportedMaxOutputTokens => Some("summary".to_string()),
        };
        Ok(CompletionResponse {
            response_id: None,
            content: content.clone(),
            raw_content: content,
            reasoning_content: None,
            tool_calls: Vec::new(),
            hosted_web_search_calls: Vec::new(),
            responses_context_items: Vec::new(),
            orchestration: Default::default(),
            trace_events: Vec::new(),
            next_sequence: 0,
            usage: TokenUsage {
                prompt_tokens: 1,
                completion_tokens: 2,
                total_tokens: 3,
                cached_prompt_tokens: 0,
                cache_write_tokens: 0,
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
